//! 领域事件订阅(消费 NATS 上的跨域事件,按需自动授权)。
//!
//! - `project.created`:创建者自动成为项目 admin(项目管理域创建项目时发布)。

use futures_util::StreamExt as _;
use std::sync::Arc;

use codeplan_kernel::ids::UserId;

use crate::domain::ResourceRole;
use crate::rbac::RbacService;

/// 订阅 `codeplan.events.project.created`,给创建者授予项目 admin 角色。
/// 消费幂等:角色授予为 upsert,重复消费无副作用。
///
/// # Errors
/// 仅在 NATS 订阅建立失败时返回。
pub async fn spawn_project_created_grant(
    nats_url: &str,
    rbac: Arc<RbacService>,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut sub = client
        .subscribe("codeplan.events.project.created")
        .await
        .map_err(|e| e.to_string())?;
    println!("identity:已订阅 project.created(自动授予创建者 admin)");
    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                eprintln!("project.created 载荷解析失败");
                continue;
            };
            // 字段可能在顶层或信封的 payload 内(中继器形态)
            let field = |key: &str| -> Option<String> {
                envelope
                    .get("payload")
                    .and_then(|p| p.get(key))
                    .or_else(|| envelope.get(key))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            };
            let Some(project_id) = field("project_id").and_then(|s| uuid::Uuid::parse_str(&s).ok())
            else {
                eprintln!("project.created 缺少 project_id");
                continue;
            };
            let Some(creator) = field("creator_id").and_then(|s| uuid::Uuid::parse_str(&s).ok())
            else {
                eprintln!("project.created 缺少 creator_id");
                continue;
            };
            if let Err(e) = rbac
                .store
                .grant(
                    UserId::from_uuid(creator),
                    "project",
                    project_id,
                    ResourceRole::Admin,
                )
                .await
            {
                eprintln!("project.created 授权失败: {}", e.message());
            }
        }
    });
    Ok(())
}
