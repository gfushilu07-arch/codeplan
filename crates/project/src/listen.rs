//! 领域事件订阅(T-207 提交关联):
//! - `commit.pushed`:解析提交消息 `#KEY-N` → 建立工作项 ↔ 提交链接;
//! - `pr.merged`:PR 标题/描述/合并提交中的 `#KEY-N`,合入默认分支时
//!   自动流转到 done(GitHub close-on-merge 语义)。

use futures_util::StreamExt as _;
use std::sync::Arc;

use crate::domain::{CommitLinkRepository, NewCommitLink};
use crate::infra::{PgCommitLinkStore, PgProjectRepository, PgWorkItemRepository};
use crate::service::ProjectService;
use crate::{domain::ProjectRepository, domain::WorkItemRepository};

/// 解析文本中的工作项引用 `#KEY-N`(如 `fix #CP-12`);大小写不敏感,去重保序。
#[must_use]
pub fn parse_item_refs(text: &str) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let Some(rest) = token.strip_prefix('#') else {
            continue;
        };
        let Some((key, num)) = rest.split_once('-') else {
            continue;
        };
        // 项目 key:字母开头,字母/数字组成
        if key.is_empty() || !key.chars().next().is_some_and(char::is_alphabetic) {
            continue;
        }
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let Ok(number) = num.parse::<i64>() else {
            continue;
        };
        let norm = key.to_ascii_uppercase();
        if seen.insert((norm.clone(), number)) {
            out.push((norm, number));
        }
    }
    out
}

/// 监听器依赖。
pub struct RefLinkerDeps {
    /// 项目服务(合并流转用,含状态机/事件)。
    pub svc: Arc<ProjectService>,
    /// 项目仓储(按 key 查)。
    pub projects: Arc<PgProjectRepository>,
    /// 工作项仓储(按编号查)。
    pub items: Arc<PgWorkItemRepository>,
    /// 提交链接仓储。
    pub links: Arc<PgCommitLinkStore>,
}

/// 启动两个订阅(commit.pushed / pr.merged)。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_ref_linker(nats_url: &str, deps: Arc<RefLinkerDeps>) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;

    // commit.pushed → 建链
    let mut commits_sub = client
        .subscribe("codeplan.events.commit.pushed")
        .await
        .map_err(|e| e.to_string())?;
    let d1 = Arc::clone(&deps);
    tokio::spawn(async move {
        while let Some(msg) = commits_sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            handle_commit_pushed(&d1, &env).await;
        }
    });

    // pr.merged → 自动流转
    let mut merged_sub = client
        .subscribe("codeplan.events.pr.merged")
        .await
        .map_err(|e| e.to_string())?;
    let d2 = Arc::clone(&deps);
    tokio::spawn(async move {
        while let Some(msg) = merged_sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            handle_pr_merged(&d2, &env).await;
        }
    });

    println!("project:已订阅 commit.pushed / pr.merged(提交关联 + 合并流转)");
    Ok(())
}

/// 信封字段读取(对象;兼容顶层与 payload 嵌套)。
fn payload_val<'a>(env: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    env.get("payload")
        .and_then(|p| p.get(key))
        .or_else(|| env.get(key))
}

fn payload_str(env: &serde_json::Value, key: &str) -> Option<String> {
    env.get("payload")
        .and_then(|p| p.get(key))
        .or_else(|| env.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

async fn handle_commit_pushed(deps: &RefLinkerDeps, env: &serde_json::Value) {
    let Some(full_name) = payload_str(env, "full_name") else {
        return;
    };
    let repo_id = payload_str(env, "repo_id").and_then(|s| uuid::Uuid::parse_str(&s).ok());
    let Some(repo_id) = repo_id else {
        return;
    };
    let Some(commits) = payload_val(env, "commits").and_then(|c| c.as_array()) else {
        return;
    };
    for commit in commits {
        let Some(sha) = commit.get("sha").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(message) = commit.get("message").and_then(|v| v.as_str()) else {
            continue;
        };
        let ref_name = commit
            .get("ref")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        for (key, number) in parse_item_refs(message) {
            let Ok(Some(project)) = deps.projects.find_by_key(&key).await else {
                continue;
            };
            let Ok(Some(item)) = deps.items.find_by_number(project.id, number).await else {
                continue; // 编号不存在:静默跳过(消息里可能引用任意数字)
            };
            let _ = deps
                .links
                .link(&NewCommitLink {
                    work_item_id: item.id,
                    repo_id,
                    full_name: full_name.clone(),
                    commit_sha: sha.to_owned(),
                    subject: message.lines().next().unwrap_or_default().to_owned(),
                    ref_name: ref_name.to_owned(),
                })
                .await;
        }
    }
}

async fn handle_pr_merged(deps: &RefLinkerDeps, env: &serde_json::Value) {
    // 仅合入默认分支才流转(GitHub close-on-merge 语义)
    if payload_val(env, "merged_to_default").and_then(|v| v.as_bool()) != Some(true) {
        return;
    }
    let mut text = String::new();
    for key in ["title", "body", "merge_commit"] {
        if let Some(v) = payload_str(env, key) {
            text.push_str(&v);
            text.push('\n');
        }
    }
    for (key, number) in parse_item_refs(&text) {
        let Ok(Some(project)) = deps.projects.find_by_key(&key).await else {
            continue;
        };
        let Ok(Some(item)) = deps.items.find_by_number(project.id, number).await else {
            continue;
        };
        // 目标按类型:bug → resolved(修复待验证);其余 → done
        let target = if item.item_type == crate::domain::ItemType::Bug {
            "resolved"
        } else {
            "done"
        };
        if item.status == target {
            continue;
        }
        if let Err(e) = deps.svc.transition_system(item.id, target).await {
            eprintln!("[ref-linker] 流转 {key}-{number} → {target} 失败: {e}");
        }
    }
}

/// 供 API 反向展示:工作项关联提交(委托仓储)。
///
/// # Errors
/// 存储故障。
pub async fn item_commits(
    links: &PgCommitLinkStore,
    work_item_id: crate::domain::WorkItemId,
) -> Result<Vec<crate::domain::WorkItemCommit>, String> {
    links
        .list_by_item(work_item_id)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_item_refs_basic() {
        let refs = parse_item_refs("fix #CP-12 and closes #cp-3, see #other-9 #12 #CP-12");
        assert_eq!(
            refs,
            vec![
                ("CP".to_owned(), 12),
                ("CP".to_owned(), 3),
                ("OTHER".to_owned(), 9),
            ]
        );
    }

    #[test]
    fn parse_item_refs_rejects_noise() {
        assert!(parse_item_refs("no refs here #123 #x- # -4 #-5").is_empty());
        assert!(parse_item_refs("").is_empty());
    }
}
