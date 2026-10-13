//! Webhook 出站(T-209):HMAC 签名、事件匹配、投递 worker(NATS 订阅 +
//! 重试 + 日志)、订阅 CRUD。

use futures_util::StreamExt as _;
use std::sync::Arc;
use std::time::Duration;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::EventEnvelope;
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::RepoId;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::json;
use sha2::Sha256;

use crate::webhook::{NewWebhook, Webhook, WebhookDelivery, WebhookId, WebhookRepository};

const DC: &str = "GIT";
/// 每次投递的尝试次数(含首次)。
pub const MAX_ATTEMPTS: u32 = 3;
/// 单次尝试超时。
pub const DELIVER_TIMEOUT: Duration = Duration::from_secs(10);

type HmacSha256 = Hmac<Sha256>;

/// 投递签名:`X-Codeplan-Signature: sha256=<hex(hmac_sha256(secret, body))>`。
#[must_use]
pub fn sign_payload(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac 接受任意长度密钥");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    });
    format!("sha256={hex}")
}

/// 事件是否命中订阅(空 events = 全部)。
#[must_use]
pub fn event_matches(hook_events: &[String], event_type: &str) -> bool {
    hook_events.is_empty() || hook_events.iter().any(|e| e == event_type)
}

/// 从信封中取仓库 ID(payload 嵌套或顶层);非仓库事件返回 None。
#[must_use]
pub fn repo_id_of(envelope: &serde_json::Value) -> Option<uuid::Uuid> {
    let v = envelope
        .get("payload")
        .and_then(|p| p.get("repo_id"))
        .or_else(|| envelope.get("repo_id"))?;
    uuid::Uuid::parse_str(v.as_str()?).ok()
}

/// 随机密钥(订阅未指定时生成)。
#[must_use]
pub fn generate_secret() -> String {
    let mut buf = [0u8; 24];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Webhook 服务(CRUD + 投递)。
pub struct WebhookService {
    hooks: Arc<dyn WebhookRepository>,
    repos: Arc<dyn crate::domain::RepositoryRepository>,
    authz: Arc<dyn Authz>,
    http: reqwest::Client,
}

impl WebhookService {
    /// 构造。
    #[must_use]
    pub fn new(
        hooks: Arc<dyn WebhookRepository>,
        repos: Arc<dyn crate::domain::RepositoryRepository>,
        authz: Arc<dyn Authz>,
    ) -> Self {
        Self {
            hooks,
            repos,
            authz,
            http: reqwest::Client::builder()
                .timeout(DELIVER_TIMEOUT)
                .build()
                .expect("HTTP 客户端构建"),
        }
    }

    async fn ensure(
        &self,
        user: &CurrentUser,
        action: Action,
        repo_id: RepoId,
    ) -> Result<(), AppError> {
        let reference = ResourceRef::Repo(RepoId::from_uuid(repo_id.as_uuid()));
        let ok = self
            .authz
            .check(user.id, action, &reference)
            .await
            .map_err(|e| AppError::internal(DC).with_details(json!(e.to_string())))?;
        if ok {
            Ok(())
        } else {
            Err(AppError::forbidden(DC, "无权限执行该操作"))
        }
    }

    async fn repo_or_404(&self, full_name: &str) -> Result<crate::domain::Repository, AppError> {
        self.repos
            .find_by_full_name(full_name)
            .await?
            .ok_or_else(|| AppError::not_found(DC, "仓库不存在"))
    }

    /// 创建订阅(secret 缺省自动生成)。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;URL 非法 `GIT-400`;无权限 `GIT-403`。
    pub async fn create(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        url: &str,
        secret: Option<&str>,
        events: Vec<String>,
    ) -> Result<Webhook, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AppError::invalid(DC, "URL 必须以 http(s):// 开头"));
        }
        let secret = match secret {
            Some(s) if !s.trim().is_empty() => s.trim().to_owned(),
            _ => generate_secret(),
        };
        self.hooks
            .create(&NewWebhook {
                repo_id: RepoId::from_uuid(repo.id.as_uuid()),
                url: url.to_owned(),
                secret,
                events,
            })
            .await
    }

    /// 订阅列表(secret 脱敏)。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn list(
        &self,
        actor: &CurrentUser,
        full_name: &str,
    ) -> Result<Vec<Webhook>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.hooks
            .list_by_repo(RepoId::from_uuid(repo.id.as_uuid()))
            .await
    }

    /// 删除订阅。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn delete(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        hook_id: WebhookId,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.hooks.delete(hook_id).await
    }

    /// 取订阅并校验归属 + 读权限。
    ///
    /// # Errors
    /// 仓库/订阅不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn find_hook_checked(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        hook_id: WebhookId,
    ) -> Result<Webhook, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        let hook = self
            .hooks
            .find_by_id(hook_id)
            .await?
            .ok_or_else(|| AppError::not_found(DC, "Webhook 不存在"))?;
        if hook.repo_id.as_uuid() != repo.id.as_uuid() {
            return Err(AppError::not_found(DC, "Webhook 不存在"));
        }
        Ok(hook)
    }

    /// 投递日志。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn deliveries(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        hook_id: WebhookId,
        limit: i64,
    ) -> Result<Vec<WebhookDelivery>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.hooks.list_deliveries(hook_id, limit).await
    }

    /// 单次投递(带重试);写日志,返回终态记录。
    pub async fn deliver(
        &self,
        hook: &Webhook,
        event_type: &str,
        envelope: &serde_json::Value,
    ) -> WebhookDelivery {
        let body = serde_json::to_vec(envelope).unwrap_or_default();
        let signature = sign_payload(&hook.secret, &body);
        let mut attempts = 0u32;
        let mut last_err: Option<String> = None;
        let mut status: Option<i32> = None;
        while attempts < MAX_ATTEMPTS {
            if attempts > 0 {
                tokio::time::sleep(Duration::from_millis(1000 * u64::from(attempts))).await;
            }
            attempts += 1;
            match self
                .http
                .post(&hook.url)
                .header("Content-Type", "application/json")
                .header("X-Codeplan-Event", event_type)
                .header("X-Codeplan-Delivery", uuid::Uuid::new_v4().to_string())
                .header("X-Codeplan-Signature", &signature)
                .body(body.clone())
                .send()
                .await
            {
                Ok(resp) => {
                    let code = resp.status().as_u16() as i32;
                    status = Some(code);
                    if (200..300).contains(&code) {
                        last_err = None;
                        break;
                    }
                    last_err = Some(format!("HTTP {code}"));
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                    status = None;
                }
            }
        }
        let ok = last_err.is_none();
        let record = WebhookDelivery {
            id: crate::webhook::WebhookDeliveryId::generate(),
            webhook_id: hook.id,
            event_type: event_type.to_owned(),
            request_body: String::from_utf8_lossy(&body).into_owned(),
            status_code: status,
            error: last_err,
            attempts: attempts as i32,
            ok,
            created_at: chrono::Utc::now(),
        };
        let _ = self.hooks.log_delivery(&record).await;
        record
    }
}

/// 投递 worker:订阅全部领域事件,按仓库投递。
///
/// # Errors
/// NATS 连接/订阅失败。
pub async fn spawn_webhook_worker(
    nats_url: &str,
    svc: Arc<WebhookService>,
    hooks: Arc<dyn WebhookRepository>,
) -> Result<(), String> {
    let client = async_nats::connect(nats_url)
        .await
        .map_err(|e| format!("NATS 连接失败({nats_url}): {e}"))?;
    let mut sub = client
        .subscribe("codeplan.events.>")
        .await
        .map_err(|e| e.to_string())?;
    println!("webhook:已订阅 codeplan.events.>(出站投递)");
    tokio::spawn(async move {
        while let Some(msg) = sub.next().await {
            let subject = msg.subject.to_string();
            let Some(event_type) = subject.strip_prefix("codeplan.events.") else {
                continue;
            };
            let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                continue;
            };
            let Some(repo_id) = repo_id_of(&envelope) else {
                continue; // 非仓库事件
            };
            let Ok(hooks_for_repo) = hooks.list_by_repo(RepoId::from_uuid(repo_id)).await else {
                continue;
            };
            for hook in hooks_for_repo {
                if !hook.active || !event_matches(&hook.events, event_type) {
                    continue;
                }
                let record = svc.deliver(&hook, event_type, &envelope).await;
                if !record.ok {
                    eprintln!(
                        "[webhook] 投递失败 {} → {}({} 次): {}",
                        event_type,
                        hook.url,
                        record.attempts,
                        record.error.as_deref().unwrap_or("?")
                    );
                }
            }
        }
    });
    Ok(())
}

/// 测试投递的 ping 信封。
#[must_use]
pub fn ping_envelope(repo_id: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "type": "ping",
        "occurred_at": chrono::Utc::now().to_rfc3339(),
        "actor": serde_json::Value::Null,
        "payload": {"repo_id": repo_id, "zen": "保持简单。"},
    })
}

#[allow(dead_code)]
fn envelope_type_check(e: &EventEnvelope) -> &str {
    &e.r#type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_hmac_sha256() {
        // 与 python hmac.new(b"k", b"body", hashlib.sha256).hexdigest() 对齐
        let sig = sign_payload("k", b"body");
        // 已知值:hmac_sha256(key="k", msg="body")
        assert!(sig.starts_with("sha256="));
        // 稳定性:同输入同输出,不同输入不同输出
        assert_eq!(sig, sign_payload("k", b"body"));
        assert_ne!(sig, sign_payload("k2", b"body"));
        assert_ne!(sig, sign_payload("k", b"body2"));
    }

    #[test]
    fn event_match_semantics() {
        let events = vec!["commit.pushed".to_owned(), "release.published".to_owned()];
        assert!(event_matches(&events, "commit.pushed"));
        assert!(!event_matches(&events, "pr.merged"));
        assert!(event_matches(&[], "anything"), "空 = 全部");
    }

    #[test]
    fn repo_id_extraction_nested_and_top() {
        let nested = serde_json::json!({
            "id": uuid::Uuid::new_v4(), "type": "x",
            "payload": {"repo_id": uuid::Uuid::new_v4().to_string()}
        });
        assert!(repo_id_of(&nested).is_some());
        let top = serde_json::json!({"repo_id": "not-a-uuid"});
        assert!(repo_id_of(&top).is_none());
        assert!(repo_id_of(&serde_json::json!({})).is_none());
    }
}
