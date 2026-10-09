//! 中继器:轮询发件箱 → 发布 NATS JetStream → 标记已发布。

use std::time::Duration;

use codeplan_common::time;

use crate::nats::subject_for;
use crate::outbox::PgOutboxStore;

/// 启动中继循环(每 [`BATCH_INTERVAL`] 一轮,单轮失败仅告警不退出)。
///
/// 返回的 Future 永不完成(除非 NATS 连接失败)。
///
/// # Errors
/// NATS 连接失败时返回错误。
pub async fn run(pool: sqlx::PgPool, nats_url: &str) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    println!("eventbus:已连接 NATS {nats_url},中继循环启动");
    let store = PgOutboxStore::new(pool);
    loop {
        match relay_once(&store, &client).await {
            Ok(n) if n > 0 => println!("eventbus:本轮中继 {n} 个事件"),
            Ok(_) => {}
            Err(e) => eprintln!("eventbus:中继失败(下轮重试): {e}"),
        }
        tokio::time::sleep(BATCH_INTERVAL).await;
    }
}

/// 轮询间隔。
const BATCH_INTERVAL: Duration = Duration::from_secs(2);

/// 单轮中继:取一批未发布事件逐个发布,全部成功后统一标记。
async fn relay_once(store: &PgOutboxStore, client: &async_nats::Client) -> Result<usize, String> {
    let rows = store
        .fetch_unpublished(200)
        .await
        .map_err(|e| e.to_string())?;
    let mut published_ids = Vec::new();
    for row in &rows {
        let subject = subject_for(&row.event_type);
        let mut payload = serde_json::json!({
            "id": row.id,
            "type": row.event_type,
            "occurred_at": time::to_rfc3339(row.created_at),
            "actor": row.actor,
            "payload": row.payload,
        });
        // 信封扁平化:actor/occurred_at 提到顶层,消费方无需二次解包
        if let Some(actor) = row.actor.clone() {
            payload["actor"] = serde_json::Value::String(actor);
        }
        client
            .publish(
                subject,
                serde_json::to_vec(&payload)
                    .map_err(|e| e.to_string())?
                    .into(),
            )
            .await
            .map_err(|e| e.to_string())?;
        published_ids.push(row.id);
    }
    if !published_ids.is_empty() {
        client.flush().await.map_err(|e| e.to_string())?;
        store
            .mark_published(&published_ids)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(published_ids.len())
}
