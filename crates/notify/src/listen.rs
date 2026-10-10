//! 领域事件订阅:映射规则(事件 → 通知草稿)→ 渠道分发(经 PluginRegistry)。
//!
//! v1 规则:
//! - `workitem.created` → 通知经办人(与操作者相同则跳过);
//! - `workitem.status_changed` → 通知报告人(与操作者相同则跳过)。

use futures_util::StreamExt as _;
use std::sync::Arc;

use codeplan_kernel::ids::UserId;

use crate::domain::{NotificationDraft, NotificationStore};

/// 信封字段读取(兼容顶层与 payload 嵌套两种形态,同 identity::listen)。
fn field(envelope: &serde_json::Value, key: &str) -> Option<String> {
    envelope
        .get("payload")
        .and_then(|p| p.get(key))
        .or_else(|| envelope.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// 映射规则(纯函数,便于测试):事件 → 通知草稿(可能为 None = 不需要通知)。
#[must_use]
pub fn notification_for_event(
    event_type: &str,
    envelope: &serde_json::Value,
    actor: Option<&str>,
) -> Option<NotificationDraft> {
    let field = |key: &str| field(envelope, key);
    let parse_user = |key: &str| -> Option<UserId> {
        field(key).and_then(|s| UserId::try_from(s.as_str()).ok())
    };
    match event_type {
        "workitem.created" => {
            let assignee = parse_user("assignee")?;
            if actor.is_some_and(|a| assignee.as_uuid().to_string() == a) {
                return None;
            }
            Some(NotificationDraft {
                recipient: assignee,
                title: format!(
                    "新工作项:{}",
                    field("title").unwrap_or_else(|| "未命名".to_owned())
                ),
                body: format!(
                    "{} 给你指派了工作项 {}",
                    actor.unwrap_or("某人"),
                    field("number").unwrap_or_default()
                ),
                link: Some(format!(
                    "/projects/{}?item={}",
                    field("project_id").unwrap_or_default(),
                    field("id").unwrap_or_default()
                )),
            })
        }
        "workitem.status_changed" => {
            let reporter = parse_user("reporter")?;
            if actor.is_some_and(|a| reporter.as_uuid().to_string() == a) {
                return None;
            }
            Some(NotificationDraft {
                recipient: reporter,
                title: format!(
                    "工作项状态变更:{} → {}",
                    field("from").unwrap_or_default(),
                    field("to").unwrap_or_default()
                ),
                body: format!(
                    "你报告的工作项 {} 状态已变更",
                    field("number").unwrap_or_default()
                ),
                link: Some(format!(
                    "/projects/{}?item={}",
                    field("project_id").unwrap_or_default(),
                    field("id").unwrap_or_default()
                )),
            })
        }
        _ => None,
    }
}

/// 订阅工作项事件并经注册表分发。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_workitem_notifications(
    nats_url: &str,
    registry: Arc<codeplan_ext::PluginRegistry>,
    store: Arc<dyn NotificationStore>,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut sub = client
        .subscribe("codeplan.events.workitem.*")
        .await
        .map_err(|e| e.to_string())?;
    println!("notify:已订阅 workitem.* 事件(通知分发)");
    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let subject = msg.subject.to_string();
            let Some(event_type) = subject.strip_prefix("codeplan.events.") else {
                continue;
            };
            let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let actor = envelope.get("actor").and_then(|v| v.as_str());
            let Some(draft) = notification_for_event(event_type, &envelope, actor) else {
                continue;
            };
            let results = registry
                .deliver_all(
                    draft.recipient,
                    &draft.title,
                    &draft.body,
                    draft.link.as_deref(),
                )
                .await;
            for (channel, result) in results {
                if let Err(e) = result {
                    eprintln!("notify:渠道 {channel} 投递失败: {e}");
                }
            }
            let _ = &store; // inbox 渠道已落库;store 预留给摘要/统计
        }
    });
    Ok(())
}
