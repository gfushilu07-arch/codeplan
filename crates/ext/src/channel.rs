//! 通知渠道扩展点(T-117):官方/第三方渠道实现此 trait,经 [`PluginRegistry`] 注册。

use async_trait::async_trait;
use codeplan_kernel::ids::UserId;
use std::sync::Arc;

/// 通知渠道:一条通知经所有已注册渠道分发(站内信/邮件/IM…)。
#[async_trait]
pub trait NotificationChannel: Send + Sync {
    /// 渠道名(如 `inbox`、`wecom`),用于审计与按渠道禁用。
    fn name(&self) -> &'static str;

    /// 投递一条通知。
    ///
    /// # Errors
    /// 投递失败由调用方决定重试/降级;单渠道失败不影响其他渠道。
    async fn deliver(
        &self,
        recipient: UserId,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> Result<(), String>;
}

/// 插件注册表:收集全部已注册渠道,批量分发。
#[derive(Default)]
pub struct PluginRegistry {
    channels: std::sync::RwLock<Vec<Arc<dyn NotificationChannel>>>,
}

impl PluginRegistry {
    /// 创建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册通知渠道(启动期调用,内置插件经此挂载)。
    pub fn register_channel(&self, channel: Arc<dyn NotificationChannel>) {
        self.channels.write().expect("插件注册锁中毒").push(channel);
    }

    /// 已注册渠道数。
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.read().expect("插件注册锁中毒").len()
    }

    /// 向全部渠道分发一条通知;返回 (渠道名, 结果) 列表供调用方记录审计/日志。
    pub async fn deliver_all(
        &self,
        recipient: UserId,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> Vec<(&'static str, Result<(), String>)> {
        let channels = self.channels.read().expect("插件注册锁中毒").clone();
        let mut results = Vec::new();
        for ch in channels {
            let name = ch.name();
            let result = ch.deliver(recipient, title, body, link).await;
            results.push((name, result));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingChannel(AtomicUsize);

    #[async_trait]
    impl NotificationChannel for CountingChannel {
        fn name(&self) -> &'static str {
            "counting"
        }

        async fn deliver(
            &self,
            _recipient: UserId,
            _title: &str,
            _body: &str,
            _link: Option<&str>,
        ) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn registry_delivers_to_all_channels() {
        let registry = PluginRegistry::new();
        let c1 = Arc::new(CountingChannel(AtomicUsize::new(0)));
        let c2 = Arc::new(CountingChannel(AtomicUsize::new(0)));
        registry.register_channel(Arc::clone(&c1) as Arc<_>);
        registry.register_channel(Arc::clone(&c2) as Arc<_>);
        assert_eq!(registry.channel_count(), 2);
        let results = registry
            .deliver_all(UserId::generate(), "标题", "正文", None)
            .await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, r)| r.is_ok()));
        assert_eq!(c1.0.load(Ordering::SeqCst), 1);
        assert_eq!(c2.0.load(Ordering::SeqCst), 1);
    }
}
