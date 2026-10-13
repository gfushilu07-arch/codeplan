//! agent gRPC 网关(T-311 / PAAS-03):平台侧实现,agent 主动外连。
//!
//! 组合根(L4)持有本实现并委托 appcenter 的主机注册表 —— L3 域之间不互相依赖,
//! gRPC 契约与装配都在 L4 完成。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use codeplan_appcenter::domain::{DeployHostId, HEARTBEAT_INTERVAL_SECS};
use codeplan_appcenter::service::AppCenterService;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// 生成的 agent 契约(与 agent 二进制共用同一 proto)。
pub mod proto {
    #![allow(clippy::doc_markdown, missing_docs)]
    tonic::include_proto!("codeplan.agent.v1");
}

use proto::agent_gateway_server::{AgentGateway, AgentGatewayServer};
use proto::{
    AgentEvent, Command, HeartbeatRequest, HeartbeatResponse, RegisterRequest, RegisterResponse,
    ReportCommandRequest, ReportCommandResponse, PushEventsResponse, WatchRequest,
};

/// 在线 agent 的命令通道表。
///
/// 表项即发送端:只要表项存在,agent 的命令流就不会结束(否则流立即关闭、
/// agent 反复重连)。T-313 将从这里向指定主机投递部署命令。
#[derive(Default)]
pub struct AgentHub {
    channels: Mutex<HashMap<uuid::Uuid, tokio::sync::mpsc::Sender<Result<Command, Status>>>>,
}

impl AgentHub {
    /// 创建空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记(或替换)某主机的命令通道。
    pub async fn attach(
        &self,
        host_id: uuid::Uuid,
        tx: tokio::sync::mpsc::Sender<Result<Command, Status>>,
    ) {
        self.channels.lock().await.insert(host_id, tx);
    }

    /// 在线 agent 数。
    pub async fn online_count(&self) -> usize {
        self.channels.lock().await.len()
    }
}

#[async_trait::async_trait]
impl codeplan_appcenter::domain::AgentCommandSink for AgentHub {
    async fn push(
        &self,
        host_id: codeplan_appcenter::domain::DeployHostId,
        kind: &str,
        payload_json: &str,
    ) -> Result<Option<String>, codeplan_common::error::AppError> {
        let command_id = uuid::Uuid::new_v4().to_string();
        let cmd = Command {
            id: command_id.clone(),
            kind: kind.to_owned(),
            payload_json: payload_json.to_owned(),
        };
        let tx = self.channels.lock().await.get(&host_id.as_uuid()).cloned();
        let Some(tx) = tx else { return Ok(None); };
        if tx.send(Ok(cmd)).await.is_err() {
            self.channels.lock().await.remove(&host_id.as_uuid());
            return Ok(None);
        }
        Ok(Some(command_id))
    }
}

/// 一条观测事件(已归一化,供前端消费)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ObservationLine {
    /// 主机 ID。
    pub host_id: String,
    /// 类型:status | log。
    pub kind: String,
    /// 内容。
    pub message: String,
    /// 时刻。
    pub at: String,
}

/// 部署观测汇聚(T-314):环形缓冲(新连上先补历史)+ 广播(在线实时收)。
///
/// 为什么两者都要:纯广播会让后连的前端看不到已发生的日志;纯缓冲则没有实时性。
#[derive(Default)]
pub struct DeployEventHub {
    buffers: Mutex<HashMap<String, std::collections::VecDeque<ObservationLine>>>,
    subs: Mutex<HashMap<String, tokio::sync::broadcast::Sender<ObservationLine>>>,
}

/// 单个部署保留的事件条数(环形上限)。
pub const OBSERVATION_BUFFER: usize = 500;

impl DeployEventHub {
    /// 创建空 hub。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入一条事件。
    pub async fn record(&self, ev: AgentEvent) {
        if ev.deployment_id.is_empty() {
            return; // 非部署期事件暂无消费方
        }
        let line = ObservationLine {
            host_id: ev.host_id,
            kind: ev.kind,
            message: ev.message,
            at: ev.at,
        };
        {
            let mut buffers = self.buffers.lock().await;
            let buf = buffers.entry(ev.deployment_id.clone()).or_default();
            if buf.len() >= OBSERVATION_BUFFER {
                buf.pop_front();
            }
            buf.push_back(line.clone());
        }
        let tx = {
            let mut subs = self.subs.lock().await;
            subs.entry(ev.deployment_id.clone())
                .or_insert_with(|| tokio::sync::broadcast::channel(256).0)
                .clone()
        };
        let _ = tx.send(line);
    }

    /// 历史快照(供新连上前端先补齐)。
    pub async fn snapshot(&self, deployment_id: &str) -> Vec<ObservationLine> {
        self.buffers
            .lock()
            .await
            .get(deployment_id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 订阅实时事件。
    pub async fn subscribe(
        &self,
        deployment_id: &str,
    ) -> tokio::sync::broadcast::Receiver<ObservationLine> {
        let mut subs = self.subs.lock().await;
        subs.entry(deployment_id.to_owned())
            .or_insert_with(|| tokio::sync::broadcast::channel(256).0)
            .subscribe()
    }
}

/// gRPC 服务实现。
pub struct AgentGatewayImpl {
    svc: Arc<AppCenterService>,
    hub: Arc<AgentHub>,
    events: Arc<DeployEventHub>,
}

impl AgentGatewayImpl {
    /// 构造。
    #[must_use]
    pub fn new(
        svc: Arc<AppCenterService>,
        hub: Arc<AgentHub>,
        events: Arc<DeployEventHub>,
    ) -> Self {
        Self { svc, hub, events }
    }

    /// 组装 tonic 服务(供组合根挂到监听器上)。
    #[must_use]
    pub fn into_service(self) -> AgentGatewayServer<Self> {
        AgentGatewayServer::new(self)
    }
}

#[tonic::async_trait]
impl AgentGateway for AgentGatewayImpl {
    async fn register(
        &self,
        req: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let r = req.into_inner();
        let host = self
            .svc
            .register_agent(&r.token, &r.hostname, &r.agent_version, r.labels)
            .await
            .map_err(|e| Status::permission_denied(e.message()))?;
        Ok(Response::new(RegisterResponse {
            host_id: host.id.as_uuid().to_string(),
            heartbeat_interval_secs: HEARTBEAT_INTERVAL_SECS,
        }))
    }

    async fn heartbeat(
        &self,
        req: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let r = req.into_inner();
        let id = uuid::Uuid::parse_str(&r.host_id)
            .map(DeployHostId::from_uuid)
            .map_err(|_| Status::invalid_argument("host_id 非法"))?;
        self.svc
            .heartbeat_agent(id)
            .await
            .map_err(|e| Status::not_found(e.message()))?;
        Ok(Response::new(HeartbeatResponse { ok: true }))
    }

    async fn report_command(
        &self,
        req: Request<ReportCommandRequest>,
    ) -> Result<Response<ReportCommandResponse>, Status> {
        let r = req.into_inner();
        match self
            .svc
            .record_agent_result(&r.command_id, r.ok, &r.message)
            .await
        {
            Ok(Some(d)) => println!(
                "[deploy] 回执 {} → 状态 {:?}({} 台目标)",
                r.command_id,
                d.status,
                d.target_hosts.len()
            ),
            Ok(None) => eprintln!("[deploy] 回执找不到对应命令:{}", r.command_id),
            Err(e) => eprintln!("[deploy] 回执处理失败:{}", e.message()),
        }
        Ok(Response::new(ReportCommandResponse { ok: true }))
    }

    async fn push_events(
        &self,
        req: Request<tonic::Streaming<AgentEvent>>,
    ) -> Result<Response<PushEventsResponse>, Status> {
        let mut stream = req.into_inner();
        let mut accepted = 0u64;
        while let Some(ev) = stream.message().await? {
            self.events.record(ev).await;
            accepted += 1;
        }
        Ok(Response::new(PushEventsResponse { accepted }))
    }

    type WatchStream = ReceiverStream<Result<Command, Status>>;

    async fn watch(
        &self,
        req: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let r = req.into_inner();
        let id = uuid::Uuid::parse_str(&r.host_id)
            .map(DeployHostId::from_uuid)
            .map_err(|_| Status::invalid_argument("host_id 非法"))?;
        // 校验主机存在,避免任意连接长期占用
        if self.svc.heartbeat_agent(id).await.is_err() {
            return Err(Status::not_found("主机不存在"));
        }
        // 命令下发通道:**发送端存进 hub**,否则发送端随作用域释放会让流立即结束,
        // agent 只能反复重连(T-313 从这里投递部署命令)。
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Command, Status>>(16);
        self.hub.attach(id.as_uuid(), tx).await;
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(stream))
    }
}

// ---------- 部署观测 WebSocket(T-314) ----------

/// 观测路由状态。
#[derive(Clone)]
pub struct ObservationState {
    /// 事件汇聚。
    pub events: Arc<DeployEventHub>,
    /// 令牌认证器(浏览器无法给 WebSocket 设头,故用查询参数带令牌)。
    pub authn: Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
}

/// 观测路由。
pub fn observation_routes(state: ObservationState) -> axum::Router<ObservationState> {
    use axum::routing::get;
    axum::Router::new()
        .route("/api/deployments/{id}/observations", get(ws_upgrade))
        .with_state(state)
}

/// 查询参数。
#[derive(serde::Deserialize)]
struct WsQuery {
    /// 会话令牌。
    token: String,
}

async fn ws_upgrade(
    ws: axum::extract::WebSocketUpgrade,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<WsQuery>,
    axum::extract::State(st): axum::extract::State<ObservationState>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    // 鉴权:令牌无效直接 401,不升级连接
    if st.authn.verify(&q.token).await.is_err() {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"code": "APP-401", "message": "令牌无效"})),
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| stream_observations(socket, id, st.events))
}

/// 先补历史快照,再持续推实时事件。
async fn stream_observations(
    mut socket: axum::extract::ws::WebSocket,
    deployment_id: String,
    hub: Arc<DeployEventHub>,
) {
    use axum::extract::ws::Message;
    let send = |line: &ObservationLine| {
        Message::Text(serde_json::to_string(line).unwrap_or_default().into())
    };
    for line in hub.snapshot(&deployment_id).await {
        if socket.send(send(&line)).await.is_err() {
            return;
        }
    }
    let mut rx = hub.subscribe(&deployment_id).await;
    loop {
        match rx.recv().await {
            Ok(line) => {
                if socket.send(send(&line)).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                let _ = socket
                    .send(Message::Text(
                        format!("{{\"kind\":\"status\",\"message\":\"丢弃滞后事件 {n} 条\"}}")
                            .into(),
                    ))
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
