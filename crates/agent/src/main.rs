//! agent 入口:注册 → 心跳 → 命令流(T-311)。
//!
//! 环境变量:
//! - `CODEPLAN_AGENT_SERVER` 平台 gRPC 地址(如 `http://127.0.0.1:50051`)
//! - `CODEPLAN_AGENT_TOKEN`  一次性注册令牌(平台"登记主机"时下发)
//! - `CODEPLAN_AGENT_LABELS` 主机标签,逗号分隔(可选)

use anyhow::{Context, Result, bail};
use codeplan_agent::proto::agent_gateway_client::AgentGatewayClient;
use codeplan_agent::deploy::{DeployPlan, EventSink};
use codeplan_agent::executor::DockerRuntime;
use codeplan_agent::proto::{
    AgentEvent, Command, HeartbeatRequest, HostStatus, RegisterRequest, ReportCommandRequest,
    WatchRequest,
};
use tokio_stream::wrappers::ReceiverStream;
use codeplan_agent::{VERSION, effective_interval_secs, parse_labels};
use tokio::time::{Duration, interval};

#[tokio::main]
async fn main() -> Result<()> {
    let server = std::env::var("CODEPLAN_AGENT_SERVER")
        .unwrap_or_else(|_| "http://127.0.0.1:50051".to_owned());
    let token = std::env::var("CODEPLAN_AGENT_TOKEN")
        .context("缺少 CODEPLAN_AGENT_TOKEN(平台登记主机时下发)")?;
    if token.trim().is_empty() {
        bail!("CODEPLAN_AGENT_TOKEN 为空");
    }
    let labels = parse_labels(&std::env::var("CODEPLAN_AGENT_LABELS").unwrap_or_default());
    let hostname = hostname();

    println!("agent {VERSION} 启动,连接 {server}(主机 {hostname})");
    let mut client = AgentGatewayClient::connect(server.clone())
        .await
        .with_context(|| format!("连接平台失败:{server}"))?;

    // 注册(一次性 token → host_id)
    let reg = client
        .register(RegisterRequest {
            token: token.trim().to_owned(),
            hostname: hostname.clone(),
            agent_version: VERSION.to_owned(),
            labels: labels.clone(),
        })
        .await
        .context("注册失败")?
        .into_inner();
    let host_id = reg.host_id.clone();
    if host_id.is_empty() {
        bail!("平台未返回 host_id");
    }
    println!("注册成功:host_id={host_id},标签={labels:?}");

    // 事件推流(T-314):常驻队列 → 断线自动重连,事件不丢
    let ev_queue = std::sync::Arc::new(codeplan_agent::deploy::EventQueue::new());
    let sink = EventSink::new(std::sync::Arc::clone(&ev_queue), host_id.clone());
    let mut ev_client = client.clone();
    tokio::spawn(async move {
        loop {
            let q = std::sync::Arc::clone(&ev_queue);
            let stream = ReceiverStream::new(async_stream_from_queue(q));
            match ev_client.push_events(stream).await {
                Ok(resp) => eprintln!("事件流关闭({})", resp.into_inner().accepted),
                Err(e) => eprintln!("事件流断开,重连:{e}"),
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    // 命令流(独立任务):收到部署命令即本地收敛并回执
    let mut watch_client = client.clone();
    let report_client = client.clone();
    let watch_host = host_id.clone();
    tokio::spawn(async move {
        // 运行时按需连接(无 Docker 的环境仍可注册心跳,只是无法执行部署)
        let mut runtime: Option<DockerRuntime> = None;
        loop {
            match watch_client
                .watch(WatchRequest {
                    host_id: watch_host.clone(),
                })
                .await
            {
                Ok(resp) => {
                    let mut stream = resp.into_inner();
                    while let Ok(Some(cmd)) = stream.message().await {
                        let (ok, message, result_json) =
                            execute_command(&mut runtime, &cmd, &sink).await;
                        let mut rc = report_client.clone();
                        let req = ReportCommandRequest {
                            host_id: watch_host.clone(),
                            command_id: cmd.id.clone(),
                            ok,
                            message: message.clone(),
                            result_json,
                        };
                        match rc.report_command(req).await {
                            Ok(_) => println!("命令 {} 回执已上报:{message}", cmd.id),
                            Err(e) => eprintln!("回执上报失败:{e}"),
                        }
                    }
                    eprintln!("命令流结束,重连");
                }
                Err(e) => eprintln!("命令流建立失败:{e}"),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    // 心跳循环
    let secs = effective_interval_secs(reg.heartbeat_interval_secs);
    println!("心跳间隔 {secs} 秒");
    let mut ticker = interval(Duration::from_secs(secs));
    loop {
        ticker.tick().await;
        let req = HeartbeatRequest {
            host_id: host_id.clone(),
            status: Some(HostStatus {
                cpu_percent: 0.0,
                mem_used_mb: 0,
                mem_total_mb: 0,
                container_count: 0,
                runtime_ready: false,
            }),
        };
        match client.heartbeat(req).await {
            Ok(_) => println!("心跳已上报"),
            Err(e) => eprintln!("心跳失败:{e}"),
        }
    }
}

/// 把持久队列包装成 gRPC 事件流(队列空则等待,不结束流)。
fn async_stream_from_queue(
    q: std::sync::Arc<codeplan_agent::deploy::EventQueue>,
) -> tokio::sync::mpsc::Receiver<AgentEvent> {
    let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);
    tokio::spawn(async move {
        loop {
            match q.pop(Duration::from_secs(30)).await {
                Some(ev) => {
                    if tx.send(ev).await.is_err() {
                        break; // 平台端已断开
                    }
                }
                None => {
                    if tx.is_closed() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// 执行下发的命令(T-313):`deploy` 本地收敛,其余类型明确回执不支持。
///
/// 返回 `(是否成功, 摘要, 结构化结果 JSON)`。
async fn execute_command(
    runtime: &mut Option<DockerRuntime>,
    cmd: &Command,
    sink: &EventSink,
) -> (bool, String, String) {
    if cmd.kind != "deploy" {
        return (
            false,
            format!("不支持的命令类型:{}", cmd.kind),
            String::new(),
        );
    }
    let plan: DeployPlan = match serde_json::from_str(&cmd.payload_json) {
        Ok(p) => p,
        Err(e) => return (false, format!("部署计划解析失败:{e}"), String::new()),
    };
    if runtime.is_none() {
        match DockerRuntime::connect() {
            Ok(r) => *runtime = Some(r),
            Err(e) => return (false, format!("容器运行时不可用:{e}"), String::new()),
        }
    }
    let Some(rt) = runtime.as_ref() else {
        return (false, "容器运行时不可用".to_owned(), String::new());
    };
    let did = plan.deployment_id.clone();
    sink.status(
        &did,
        &format!("开始部署 {} 个容器({} 个副本)", plan.containers.len(), plan.containers.len()),
    )
    .await;
    match rt.execute(&plan).await {
        Ok(outcome) => {
            for c in &outcome.containers {
                sink.status(&did, &format!("容器 {} {}", c.name, c.detail)).await;
            }
            // 日志跟随放**后台**:它最长 30 秒,若阻塞在返回前会把回执一起推迟,
            // 平台侧部署状态就会莫名慢半分钟(超时收敛也可能误伤)。
            let logs_rt = rt.clone();
            let logs_sink = sink.clone();
            let names: Vec<String> = outcome.containers.iter().map(|c| c.name.clone()).collect();
            tokio::spawn(async move {
                for name in names {
                    logs_rt.follow_logs(&name, &did, &logs_sink).await;
                }
            });
            let json = serde_json::to_string(&outcome).unwrap_or_default();
            (outcome.all_healthy(), outcome.summary(), json)
        }
        Err(e) => {
            sink.status(&did, &format!("部署失败:{e}")).await;
            (false, format!("部署失败:{e}"), String::new())
        }
    }
}

/// 主机名(取不到时用 `unknown`)。
fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}
