//! 搜索管线(T-213):PG 全量重建 + 领域事件增量更新。

use std::sync::Arc;

use futures_util::StreamExt as _;
use sqlx::PgPool;

use crate::index::{DocKind, SearchDoc, SearchService};

/// 全量重建:工作项 + Wiki 页(只读投影)。
///
/// # Errors
/// 存储查询失败;索引写入失败。
pub async fn rebuild_all(pool: &PgPool, svc: &SearchService) -> Result<usize, String> {
    let wi_rows: Vec<(uuid::Uuid, String, String, uuid::Uuid)> = sqlx::query_as(
        "SELECT w.id, w.title, w.description, w.project_id
         FROM proj_work_item w",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let wiki_rows: Vec<(uuid::Uuid, String, String, uuid::Uuid)> =
        sqlx::query_as("SELECT p.id, p.title, p.content, p.project_id FROM wiki_page p")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;

    let mut docs: Vec<SearchDoc> = Vec::with_capacity(wi_rows.len() + wiki_rows.len());
    for (id, title, body, project_id) in wi_rows {
        docs.push(SearchDoc {
            id: format!("workitem:{id}"),
            kind: DocKind::WorkItem,
            project_id: Some(project_id),
            title,
            body,
        });
    }
    for (id, title, body, project_id) in wiki_rows {
        docs.push(SearchDoc {
            id: format!("wiki:{id}"),
            kind: DocKind::WikiPage,
            project_id: Some(project_id),
            title,
            body,
        });
    }
    let n = docs.len();
    svc.rebuild(&docs).map_err(|e| e.message().to_owned())?;
    Ok(n)
}

/// 事件 worker:workitem.*/wiki.* → upsert(简化:每次事件按 payload id 重读投影)。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_search_worker(
    nats_url: &str,
    pool: PgPool,
    svc: Arc<SearchService>,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut sub = client
        .subscribe("codeplan.events.workitem.*")
        .await
        .map_err(|e| e.to_string())?;
    let mut wiki_sub = client
        .subscribe("codeplan.events.wiki.*")
        .await
        .map_err(|e| e.to_string())?;
    println!("search:已订阅 workitem.*/wiki.*(索引增量)");

    let svc2 = Arc::clone(&svc);
    let pool2 = pool.clone();
    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let Some(id) = payload_field(&env, "id").and_then(|v| uuid::Uuid::parse_str(&v).ok())
            else {
                continue;
            };
            let Ok(rows) = sqlx::query_as::<_, (String, String, uuid::Uuid)>(
                "SELECT title, description, project_id FROM proj_work_item WHERE id = $1",
            )
            .bind(id)
            .fetch_all(&pool2)
            .await
            else {
                continue;
            };
            for (title, body, project_id) in rows {
                let _ = svc2.upsert(&[SearchDoc {
                    id: format!("workitem:{id}"),
                    kind: DocKind::WorkItem,
                    project_id: Some(project_id),
                    title,
                    body,
                }]);
            }
        }
    });

    let pool3 = pool;
    tokio::spawn(async move {
        while let Some(msg) = wiki_sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let Some(id) = payload_field(&env, "id").and_then(|v| uuid::Uuid::parse_str(&v).ok())
            else {
                continue;
            };
            let Ok(rows) = sqlx::query_as::<_, (String, String, uuid::Uuid)>(
                "SELECT title, content, project_id FROM wiki_page WHERE id = $1",
            )
            .bind(id)
            .fetch_all(&pool3)
            .await
            else {
                continue;
            };
            for (title, body, project_id) in rows {
                let _ = svc.upsert(&[SearchDoc {
                    id: format!("wiki:{id}"),
                    kind: DocKind::WikiPage,
                    project_id: Some(project_id),
                    title,
                    body,
                }]);
            }
        }
    });
    Ok(())
}

fn payload_field(env: &serde_json::Value, key: &str) -> Option<String> {
    env.get("payload")
        .and_then(|p| p.get(key))
        .or_else(|| env.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

// ---------- 代码索引(T-306) ----------

/// 全量重建代码索引:遍历未归档仓库的默认分支。
///
/// 说明:`repo_repository` 属 git-service 域,这里只做**只读投影**查询
/// (与 ci 域定位仓库同一既有做法),不写他域数据。
///
/// # Errors
/// 存储查询失败;git 调用失败。
pub async fn rebuild_code(
    pool: &PgPool,
    svc: &SearchService,
    repo_root: &str,
) -> Result<usize, String> {
    let repos: Vec<(uuid::Uuid, String, String)> = sqlx::query_as(
        "SELECT id, full_name, default_branch FROM repo_repository WHERE archived = false",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut total = 0usize;
    for (repo_id, full_name, branch) in repos {
        let bare = std::path::PathBuf::from(format!("{repo_root}/{full_name}.git"));
        if !bare.exists() {
            continue;
        }
        let reference = if branch.is_empty() { "HEAD" } else { &branch };
        match crate::code::docs_for_ref(&bare, repo_id, reference).await {
            Ok(docs) => {
                total += docs.len();
                let _ = svc.upsert(&docs);
            }
            Err(e) => eprintln!("search:代码索引跳过 {full_name}({reference}):{e}"),
        }
    }
    Ok(total)
}

/// 代码增量 worker:commit.pushed → 索引本次变更的文本文件。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_code_index_worker(
    nats_url: &str,
    svc: Arc<SearchService>,
    repo_root: String,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut sub = client
        .subscribe("codeplan.events.commit.pushed")
        .await
        .map_err(|e| e.to_string())?;
    println!("search:已订阅 commit.pushed(代码索引增量)");

    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let (Some(repo_id), Some(full_name)) = (
                payload_field(&env, "repo_id").and_then(|v| uuid::Uuid::parse_str(&v).ok()),
                payload_field(&env, "full_name"),
            ) else {
                continue;
            };
            // refs 为钩子原始行 `old new ref`;取每行 new 作为待索引提交
            let Some(refs) = env
                .get("payload")
                .and_then(|p| p.get("refs"))
                .or_else(|| env.get("refs"))
                .and_then(|v| v.as_array())
            else {
                continue;
            };
            let bare = std::path::PathBuf::from(format!("{repo_root}/{full_name}.git"));
            for line in refs.iter().filter_map(|v| v.as_str()) {
                let mut it = line.split_whitespace();
                let (_, Some(new), _) = (it.next(), it.next(), it.next()) else {
                    continue;
                };
                if new.chars().all(|c| c == '0') {
                    continue; // 删除引用
                }
                match crate::code::docs_for_commit(&bare, repo_id, new).await {
                    Ok(docs) if !docs.is_empty() => {
                        let _ = svc.upsert(&docs);
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("search:代码增量失败 {full_name}@{new}:{e}"),
                }
            }
        }
    });
    Ok(())
}
