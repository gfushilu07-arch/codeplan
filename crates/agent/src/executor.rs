//! 本地容器运行时(T-313 / PAAS-04):bollard 拉镜像、重建容器、健康检查。
//!
//! 幂等策略:**同名容器先移除再创建**(期望状态收敛),重复下发不会叠加实例。

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, RemoveContainerOptionsBuilder,
    StartContainerOptionsBuilder,
};
use futures_util::StreamExt as _;

use crate::deploy::{
    ContainerOutcome, ContainerPlan, DeployOutcome, DeployPlan, HealthPlan, health_url,
    volume_name,
};

/// 规格里的协议是**应用层**的(http/grpc),Docker 端口只认 tcp/udp,
/// 直接透传会让 `docker start` 报 `unknown protocol`(实测踩过)。
#[must_use]
pub fn docker_protocol(spec_protocol: &str) -> &'static str {
    if spec_protocol.eq_ignore_ascii_case("udp") {
        "udp"
    } else {
        "tcp"
    }
}

/// 本地容器运行时(可克隆:bollard 客户端是廉价句柄,日志跟随要放后台)。
#[derive(Clone)]
pub struct DockerRuntime {
    docker: Docker,
}

/// 探测 Docker socket(DOCKER_HOST 优先,其后各平台惯例路径)。
fn connect_docker() -> Result<Docker, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        std::env::var("DOCKER_HOST")
            .ok()
            .filter(|h| h.starts_with("unix://")),
        Some("unix:///var/run/docker.sock".to_owned()),
        Some(format!("unix://{home}/.docker/run/docker.sock")),
        Some("unix:///opt/homebrew/run/docker.sock".to_owned()),
    ];
    let mut last = String::from("无候选路径");
    for cand in candidates.into_iter().flatten() {
        let path = cand.trim_start_matches("unix://");
        if !std::path::Path::new(path).exists() {
            last = format!("socket 不存在:{path}");
            continue;
        }
        match Docker::connect_with_socket(path, 120, bollard::API_DEFAULT_VERSION) {
            Ok(d) => return Ok(d),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("Docker 连接失败:{last}"))
}

impl DockerRuntime {
    /// 连接本机 Docker。
    ///
    /// # Errors
    /// 无可用 socket 或 daemon 不可达。
    pub fn connect() -> Result<Self, String> {
        Ok(Self {
            docker: connect_docker()?,
        })
    }

    /// 执行部署计划(逐容器:拉镜像 → 重建 → 启动 → 探活)。
    ///
    /// # Errors
    /// 任一容器失败即返回错误(但已成功的容器保持运行,便于排查)。
    pub async fn execute(&self, plan: &DeployPlan) -> Result<DeployOutcome, String> {
        let mut outcomes = Vec::with_capacity(plan.containers.len());
        for c in &plan.containers {
            outcomes.push(self.run_container(&plan.app, c).await?);
        }
        Ok(DeployOutcome {
            deployment_id: plan.deployment_id.clone(),
            containers: outcomes,
        })
    }

    async fn run_container(
        &self,
        app: &str,
        c: &ContainerPlan,
    ) -> Result<ContainerOutcome, String> {
        self.pull_image(&c.image).await?;
        self.remove_if_exists(&c.name).await?;
        let id = self.create_container(app, c).await?;
        self.start_container(&c.name).await?;

        let (healthy, detail) = match &c.healthcheck {
            Some(h) => match self.wait_healthy(h).await {
                Ok(true) => (true, "健康检查通过".to_owned()),
                Ok(false) => (false, "健康检查超时".to_owned()),
                Err(e) => (false, format!("健康检查失败:{e}")),
            },
            None => (true, "已启动(未配置健康检查)".to_owned()),
        };
        Ok(ContainerOutcome {
            name: c.name.clone(),
            container_id: id,
            healthy,
            detail,
        })
    }

    /// 拉取镜像(已存在时 Docker 会快速跳过)。
    async fn pull_image(&self, image: &str) -> Result<(), String> {
        let opts = CreateImageOptionsBuilder::default()
            .from_image(image)
            .build();
        let mut stream = self.docker.create_image(Some(opts), None, None);
        while let Some(item) = stream.next().await {
            if let Err(e) = item {
                return Err(format!("拉取镜像 {image} 失败:{e}"));
            }
        }
        Ok(())
    }

    /// 移除同名容器(不存在视为成功)。
    async fn remove_if_exists(&self, name: &str) -> Result<(), String> {
        let opts = RemoveContainerOptionsBuilder::default().force(true).build();
        match self.docker.remove_container(name, Some(opts)).await {
            Ok(()) => Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(format!("移除旧容器 {name} 失败:{e}")),
        }
    }

    async fn create_container(&self, app: &str, c: &ContainerPlan) -> Result<String, String> {
        let mut exposed: Vec<String> = Vec::new();
        let mut bindings: HashMap<String, Option<Vec<bollard::models::PortBinding>>> =
            HashMap::new();
        for p in &c.ports {
            let key = format!("{}/{}", p.container, docker_protocol(&p.protocol));
            exposed.push(key.clone());
            bindings.insert(
                key,
                Some(vec![bollard::models::PortBinding {
                    host_ip: Some("0.0.0.0".to_owned()),
                    host_port: Some(p.container.to_string()),
                }]),
            );
        }
        let binds: Vec<String> = c
            .volumes
            .iter()
            .map(|v| format!("{}:{}", volume_name(app, &v.name), v.mount_path))
            .collect();
        let env: Vec<String> = c.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        let config = ContainerCreateBody {
            image: Some(c.image.clone()),
            env: Some(env),
            exposed_ports: Some(exposed),
            host_config: Some(bollard::models::HostConfig {
                port_bindings: if bindings.is_empty() { None } else { Some(bindings) },
                binds: if binds.is_empty() { None } else { Some(binds) },
                restart_policy: Some(bollard::models::RestartPolicy {
                    name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                    maximum_retry_count: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptionsBuilder::default().name(&c.name).build()),
                config,
            )
            .await
            .map_err(|e| format!("创建容器 {} 失败:{e}", c.name))?;
        Ok(created.id)
    }

    async fn start_container(&self, name: &str) -> Result<(), String> {
        self.docker
            .start_container(name, Some(StartContainerOptionsBuilder::default().build()))
            .await
            .map_err(|e| format!("启动容器 {name} 失败:{e}"))
    }

    /// 跟随容器日志并推给平台(最多 `LOG_FOLLOW_SECS` 秒)。
    ///
    /// 用 `follow: true` + `tail: 20` 先补最近输出再实时跟随;超时后主动结束,
    /// 避免长连接无限占用(部署日志的"实时"需求在启动阶段最强)。
    pub async fn follow_logs(
        &self,
        name: &str,
        deployment_id: &str,
        sink: &crate::deploy::EventSink,
    ) {
        use bollard::query_parameters::LogsOptionsBuilder;
        let opts = LogsOptionsBuilder::default()
            .follow(true)
            .stdout(true)
            .stderr(true)
            .tail("20")
            .build();
        let mut stream = self.docker.logs(name, Some(opts));
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(crate::deploy::LOG_FOLLOW_SECS);
        loop {
            let next = tokio::time::timeout_at(deadline, stream.next()).await;
            match next {
                Ok(Some(Ok(chunk))) => {
                    let text = match chunk {
                        bollard::container::LogOutput::StdOut { message }
                        | bollard::container::LogOutput::StdErr { message }
                        | bollard::container::LogOutput::Console { message }
                        | bollard::container::LogOutput::StdIn { message } => {
                            String::from_utf8_lossy(&message).into_owned()
                        }
                    };
                    for line in text.lines().filter(|l| !l.trim().is_empty()) {
                        sink.log(deployment_id, line).await;
                    }
                }
                Ok(Some(Err(_))) | Ok(None) => break,
                Err(_) => break, // 超时:正常结束
            }
        }
    }

    /// 探活:按 `interval` 轮询,最多等到 `timeout × interval` 或 60 秒。
    async fn wait_healthy(&self, h: &HealthPlan) -> Result<bool, String> {
        let url = health_url(h);
        let interval = u64::from(h.interval_seconds.max(1));
        let attempts = (u64::from(h.timeout_seconds.max(1)) * 60 / interval).max(3);
        for i in 0..attempts {
            match http_get_ok(&url).await {
                Ok(true) => return Ok(true),
                Ok(false) | Err(_) if i + 1 < attempts => {
                    tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                }
                Ok(false) => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        Ok(false)
    }
}

/// 极简 HTTP GET:只读取状态行判断 2xx(避免为探活引入 HTTP 客户端依赖)。
async fn http_get_ok(url: &str) -> Result<bool, String> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let rest = url.strip_prefix("http://").ok_or("仅支持 http 探活")?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_owned()),
    };
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(authority),
    )
    .await
    .map_err(|_| "连接超时".to_owned())?
    .map_err(|e| e.to_string())?;
    let mut stream = stream;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        stream.read_to_end(&mut buf),
    )
    .await;
    let text = String::from_utf8_lossy(&buf);
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok());
    Ok(status.is_some_and(|s| (200..300).contains(&s)))
}

#[cfg(test)]
mod tests {
    use super::docker_protocol;

    #[test]
    fn maps_application_protocol_to_docker_protocol() {
        assert_eq!(docker_protocol("http"), "tcp", "http 是应用层,不是 Docker 协议");
        assert_eq!(docker_protocol("grpc"), "tcp");
        assert_eq!(docker_protocol("tcp"), "tcp");
        assert_eq!(docker_protocol(""), "tcp", "缺省 tcp");
        assert_eq!(docker_protocol("UDP"), "udp");
    }

    #[tokio::test]
    async fn http_get_ok_rejects_non_http() {
        assert!(super::http_get_ok("https://example.com").await.is_err());
    }

    #[tokio::test]
    async fn http_get_ok_false_when_nothing_listening() {
        // 高位端口无服务:应返回 Err(连接失败)而不是 panic
        assert!(super::http_get_ok("http://127.0.0.1:59999/healthz").await.is_err());
    }
}
