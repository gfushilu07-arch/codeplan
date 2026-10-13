//! 部署命令执行(T-313 / PAAS-04):拉镜像 → 起容器 → 健康检查。
//!
//! 期望状态由平台下达(`DeployPlan`),agent **只做本地收敛**:同名旧容器先移除
//! 再按计划重建,保证重复下发幂等。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 部署计划(平台下发)。
#[derive(Debug, Clone, Deserialize)]
pub struct DeployPlan {
    /// 部署记录 ID。
    pub deployment_id: String,
    /// 应用键。
    pub app: String,
    /// 环境名。
    pub env: String,
    /// 容器计划(副本数即条目数)。
    pub containers: Vec<ContainerPlan>,
}

/// 单容器计划。
#[derive(Debug, Clone, Deserialize)]
pub struct ContainerPlan {
    /// 容器名(平台生成,全局唯一)。
    pub name: String,
    /// 镜像(含 tag)。
    pub image: String,
    /// 端口。
    #[serde(default)]
    pub ports: Vec<PortPlan>,
    /// 环境变量(**已由平台注入配置与密钥的明文**;agent 不落盘)。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 卷。
    #[serde(default)]
    pub volumes: Vec<VolumePlan>,
    /// 健康检查。
    #[serde(default)]
    pub healthcheck: Option<HealthPlan>,
}

/// 端口计划。
#[derive(Debug, Clone, Deserialize)]
pub struct PortPlan {
    /// 容器端口。
    pub container: u16,
    /// 协议。
    #[serde(default)]
    pub protocol: String,
}

/// 卷计划。
#[derive(Debug, Clone, Deserialize)]
pub struct VolumePlan {
    /// 逻辑卷名。
    pub name: String,
    /// 容器内挂载路径。
    pub mount_path: String,
}

/// 健康检查计划。
#[derive(Debug, Clone, Deserialize)]
pub struct HealthPlan {
    /// 探活路径。
    pub path: String,
    /// 端口。
    pub port: u16,
    /// 间隔秒。
    #[serde(default = "default_interval")]
    pub interval_seconds: u32,
    /// 超时秒。
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

fn default_interval() -> u32 {
    10
}
fn default_timeout() -> u32 {
    3
}

/// 单容器执行结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainerOutcome {
    /// 容器名。
    pub name: String,
    /// 容器 ID。
    pub container_id: String,
    /// 是否通过健康检查(无健康检查时启动即视为通过)。
    pub healthy: bool,
    /// 说明。
    pub detail: String,
}

/// 部署结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeployOutcome {
    /// 部署记录 ID。
    pub deployment_id: String,
    /// 各容器结果。
    pub containers: Vec<ContainerOutcome>,
}

impl DeployOutcome {
    /// 是否全部成功。
    #[must_use]
    pub fn all_healthy(&self) -> bool {
        self.containers.iter().all(|c| c.healthy)
    }

    /// 摘要文本(回执 message)。
    #[must_use]
    pub fn summary(&self) -> String {
        let ok = self.containers.iter().filter(|c| c.healthy).count();
        format!("{}/{} 容器就绪", ok, self.containers.len())
    }
}

/// 卷的宿主侧名称(带应用前缀,避免跨应用撞名)。
#[must_use]
pub fn volume_name(app: &str, plan_name: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
    };
    format!("codeplan-{}-{}", clean(app), clean(plan_name))
}

/// 健康检查 URL。
#[must_use]
pub fn health_url(plan: &HealthPlan) -> String {
    let path = if plan.path.starts_with('/') {
        plan.path.clone()
    } else {
        format!("/{}", plan.path)
    };
    format!("http://127.0.0.1:{}{path}", plan.port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plan_with_defaults() {
        let json = r#"{
            "deployment_id":"d1","app":"web-portal","env":"dev",
            "containers":[{"name":"c1","image":"img:1.0"}]
        }"#;
        let plan: DeployPlan = serde_json::from_str(json).expect("应解析");
        assert_eq!(plan.containers.len(), 1);
        let c = &plan.containers[0];
        assert!(c.ports.is_empty() && c.env.is_empty() && c.volumes.is_empty());
        assert!(c.healthcheck.is_none(), "健康检查可选");
    }

    #[test]
    fn volume_name_is_prefixed_and_sanitized() {
        assert_eq!(volume_name("web-portal", "data"), "codeplan-web-portal-data");
        assert_eq!(volume_name("a_b", "x.y"), "codeplan-a-b-x-y");
    }

    #[test]
    fn health_url_normalizes_path() {
        let h = HealthPlan {
            path: "healthz".to_owned(),
            port: 8080,
            interval_seconds: 5,
            timeout_seconds: 2,
        };
        assert_eq!(health_url(&h), "http://127.0.0.1:8080/healthz");
        let h2 = HealthPlan {
            path: "/ready".to_owned(),
            ..h
        };
        assert_eq!(health_url(&h2), "http://127.0.0.1:8080/ready");
    }

    #[tokio::test]
    async fn event_queue_fifo_and_bounded() {
        let q = EventQueue::new();
        assert!(q.is_empty());
        for i in 0..3 {
            q.push(crate::proto::AgentEvent {
                host_id: "h".to_owned(),
                deployment_id: "d".to_owned(),
                kind: EVENT_LOG.to_owned(),
                message: format!("line-{i}"),
                at: String::new(),
            });
        }
        assert_eq!(q.len(), 3);
        assert_eq!(q.pop(std::time::Duration::from_millis(10)).await.expect("取件").message, "line-0", "先进先出");
        // 超限丢弃最旧,保证队列有界
        for i in 0..(EVENT_QUEUE_CAP + 10) {
            q.push(crate::proto::AgentEvent {
                host_id: "h".to_owned(),
                deployment_id: "d".to_owned(),
                kind: EVENT_LOG.to_owned(),
                message: format!("m{i}"),
                at: String::new(),
            });
        }
        assert_eq!(q.len(), EVENT_QUEUE_CAP, "队列有界");
    }

    #[tokio::test]
    async fn event_sink_emits_status_and_log() {
        let q = std::sync::Arc::new(EventQueue::new());
        let sink = EventSink::new(std::sync::Arc::clone(&q), "host-1".to_owned());
        sink.status("dep-1", "开始部署").await;
        sink.log("dep-1", "hello").await;
        let a = q.pop(std::time::Duration::from_millis(10)).await.expect("取件");
        assert_eq!(a.kind, EVENT_STATUS);
        assert_eq!(a.host_id, "host-1");
        let b = q.pop(std::time::Duration::from_millis(10)).await.expect("取件");
        assert_eq!(b.kind, EVENT_LOG);
        assert_eq!(b.message, "hello");
    }

    #[test]
    fn outcome_summary_and_health() {
        let o = DeployOutcome {
            deployment_id: "d1".to_owned(),
            containers: vec![
                ContainerOutcome {
                    name: "a".to_owned(),
                    container_id: "1".to_owned(),
                    healthy: true,
                    detail: String::new(),
                },
                ContainerOutcome {
                    name: "b".to_owned(),
                    container_id: "2".to_owned(),
                    healthy: false,
                    detail: "未通过探活".to_owned(),
                },
            ],
        };
        assert!(!o.all_healthy());
        assert_eq!(o.summary(), "1/2 容器就绪");
    }
}

/// 事件类型:状态。
pub const EVENT_STATUS: &str = "status";
/// 事件类型:日志。
pub const EVENT_LOG: &str = "log";

/// 事件上报口(T-314):执行期把进度与日志推给平台。
///
/// 内部是**持久队列 + 通知**:gRPC 事件流断开后重连仍能从同一队列继续取,
/// 不会像把接收端 move 进流那样"一次断开就永久失效"。
#[derive(Clone)]
pub struct EventSink {
    inner: std::sync::Arc<EventQueue>,
    host_id: String,
}

/// 事件队列(进程内)。
#[derive(Default)]
pub struct EventQueue {
    q: std::sync::Mutex<std::collections::VecDeque<crate::proto::AgentEvent>>,
    notify: tokio::sync::Notify,
}

/// 队列上限(超出丢弃最旧,避免平台长时间不可达时无限堆积)。
pub const EVENT_QUEUE_CAP: usize = 2000;

impl EventQueue {
    /// 创建空队列。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队并唤醒取件方。
    pub fn push(&self, ev: crate::proto::AgentEvent) {
        if let Ok(mut q) = self.q.lock() {
            if q.len() >= EVENT_QUEUE_CAP {
                q.pop_front();
            }
            q.push_back(ev);
        }
        self.notify.notify_one();
    }

    /// 取出一条(无则等待 `timeout`)。
    pub async fn pop(
        &self,
        timeout: std::time::Duration,
    ) -> Option<crate::proto::AgentEvent> {
        let take = || self.q.lock().ok().and_then(|mut q| q.pop_front());
        if let Some(ev) = take() {
            return Some(ev);
        }
        let _ = tokio::time::timeout(timeout, self.notify.notified()).await;
        take()
    }

    /// 待发送条数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.q.lock().map(|q| q.len()).unwrap_or(0)
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl EventSink {
    /// 构造(共享队列)。
    #[must_use]
    pub fn new(inner: std::sync::Arc<EventQueue>, host_id: String) -> Self {
        Self { inner, host_id }
    }

    /// 上报一条状态。
    pub async fn status(&self, deployment_id: &str, message: &str) {
        self.emit(EVENT_STATUS, deployment_id, message);
    }

    /// 上报一行日志。
    pub async fn log(&self, deployment_id: &str, line: &str) {
        self.emit(EVENT_LOG, deployment_id, line);
    }

    fn emit(&self, kind: &str, deployment_id: &str, message: &str) {
        self.inner.push(crate::proto::AgentEvent {
            host_id: self.host_id.clone(),
            deployment_id: deployment_id.to_owned(),
            kind: kind.to_owned(),
            message: message.to_owned(),
            at: chrono::Utc::now().to_rfc3339(),
        });
    }
}

/// 日志跟随时长上限(秒):够看到启动输出,又不会让流无限占用。
pub const LOG_FOLLOW_SECS: u64 = 30;
