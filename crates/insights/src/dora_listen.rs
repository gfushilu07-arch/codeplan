//! DORA 事实采集(T-307):订阅领域事件写入 `ins_dora_fact`。
//!
//! 本域为**横切只读投影**(与报表同一职责):只读 `repo_repository` /
//! `rev_pull_request` 取默认分支与 PR 创建时间,不写他域数据。
//! 订阅方按 at-least-once 设计,写入靠唯一索引幂等。

use std::sync::Arc;

use futures_util::StreamExt as _;
use sqlx::PgPool;

use crate::dora::{FactKind, PgDoraStore};

fn field<'a>(env: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    env.get("payload")
        .and_then(|p| p.get(key))
        .or_else(|| env.get(key))
}

fn field_str(env: &serde_json::Value, key: &str) -> Option<String> {
    field(env, key).and_then(|v| v.as_str()).map(str::to_owned)
}

/// 仓库默认分支(只读投影)。
async fn default_branch(pool: &PgPool, repo_id: uuid::Uuid) -> Option<String> {
    sqlx::query_as::<_, (String,)>("SELECT default_branch FROM repo_repository WHERE id = $1")
        .bind(repo_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(b,)| b)
}

/// 启动采集 worker。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_dora_worker(
    nats_url: &str,
    pool: PgPool,
    store: Arc<PgDoraStore>,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut ci_sub = client
        .subscribe("codeplan.events.ci.run.finished")
        .await
        .map_err(|e| e.to_string())?;
    let mut pr_sub = client
        .subscribe("codeplan.events.pr.merged")
        .await
        .map_err(|e| e.to_string())?;
    println!("insights:已订阅 ci.run.finished / pr.merged(DORA 事实采集)");

    // 默认分支运行 → deploy / failure
    let pool_ci = pool.clone();
    let store_ci = Arc::clone(&store);
    tokio::spawn(async move {
        while let Some(msg) = ci_sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let (Some(repo_id), Some(state), Some(branch)) = (
                field_str(&env, "repo_id").and_then(|v| uuid::Uuid::parse_str(&v).ok()),
                field_str(&env, "state"),
                field_str(&env, "branch"),
            ) else {
                continue;
            };
            // 只统计默认分支:其余分支的运行不是部署
            let Some(default) = default_branch(&pool_ci, repo_id).await else {
                continue;
            };
            if branch != default {
                continue;
            }
            let kind = match state.as_str() {
                "success" => FactKind::Deploy,
                "failure" => FactKind::Failure,
                _ => continue,
            };
            let sha = field_str(&env, "sha").unwrap_or_default();
            let pipeline = field_str(&env, "pipeline").unwrap_or_default();
            // 引用用 sha+pipeline 组合:同一提交的多次流水线各自计数
            let reference = format!("{sha}:{pipeline}");
            let _ = store_ci
                .insert(&crate::dora::NewFact {
                    repo_id,
                    kind,
                    occurred_at: chrono::Utc::now(),
                    lead_time_seconds: None,
                    subject: format!("{pipeline} · {state}"),
                    reference,
                    detail: serde_json::json!({"sha": sha, "pipeline": pipeline, "state": state}),
                })
                .await;
        }
    });

    // 合入默认分支的 PR → change(带前置时间)
    let pool_pr = pool;
    let store_pr = store;
    tokio::spawn(async move {
        while let Some(msg) = pr_sub.next().await {
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            if !field(&env, "merged_to_default")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                continue; // 只统计合入默认分支的变更
            }
            let Some(repo_id) =
                field_str(&env, "repo_id").and_then(|v| uuid::Uuid::parse_str(&v).ok())
            else {
                continue;
            };
            let merge_commit = field_str(&env, "merge_commit").unwrap_or_default();
            // PR 创建时间 → 合并时间 = 变更前置时间
            let lead: Option<i64> = sqlx::query_as::<_, (Option<i64>,)>(
                "SELECT EXTRACT(EPOCH FROM (now() - created_at))::BIGINT \
                 FROM rev_pull_request WHERE repo_id = $1 AND merge_commit = $2 LIMIT 1",
            )
            .bind(repo_id)
            .bind(&merge_commit)
            .fetch_optional(&pool_pr)
            .await
            .ok()
            .flatten()
            .and_then(|(v,)| v);
            let _ = store_pr
                .insert(&crate::dora::NewFact {
                    repo_id,
                    kind: FactKind::Change,
                    occurred_at: chrono::Utc::now(),
                    lead_time_seconds: lead,
                    subject: format!("merge {merge_commit}"),
                    reference: merge_commit.clone(),
                    detail: serde_json::json!({"merge_commit": merge_commit}),
                })
                .await;
        }
    });
    Ok(())
}
