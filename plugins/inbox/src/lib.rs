//! inbox 官方内置插件(T-117):站内信通知渠道——平台首个经 EXT-01
//! 扩展点接入的插件,验证插件框架本身。
//!
//! 投递 = 写入 `ntf_notification`(站内信),由 notify::api 提供读取。

use async_trait::async_trait;
use codeplan_common::error::AppError;
use codeplan_ext::NotificationChannel;
use codeplan_kernel::ids::UserId;
use codeplan_notify::domain::NotificationStore;
use std::sync::Arc;

/// 站内信渠道:通知落 `ntf_notification` 表。
pub struct InboxChannel {
    store: Arc<dyn NotificationStore>,
}

impl InboxChannel {
    /// 构造(共享通知存储)。
    #[must_use]
    pub fn new(store: Arc<dyn NotificationStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl NotificationChannel for InboxChannel {
    fn name(&self) -> &'static str {
        "inbox"
    }

    async fn deliver(
        &self,
        recipient: UserId,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> Result<(), String> {
        self.store
            .insert(
                &codeplan_notify::domain::NotificationDraft {
                    recipient,
                    title: title.to_owned(),
                    body: body.to_owned(),
                    link: link.map(str::to_owned),
                },
                self.name(),
            )
            .await
            .map_err(|e: AppError| e.message().to_owned())
    }
}
