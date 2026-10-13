//! Step 执行后端(T-302):Shell(本机进程)与 Docker(bollard 容器)双实现。

use codeplan_common::error::AppError;

/// 单步执行结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StepOutcome {
    /// 退出码(信号终止等无码场景 = -1)。
    pub exit_code: i32,
    /// 合并 stdout/stderr 输出(尾部 64KB 截断)。
    pub output: String,
}

impl StepOutcome {
    /// 是否成功。
    #[must_use]
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// 执行规格。
#[derive(Debug, Clone)]
pub struct StepSpec {
    /// shell 命令。
    pub command: String,
    /// 工作目录(宿主路径;Docker 后端挂载到 /workspace)。
    pub workdir: std::path::PathBuf,
    /// 环境变量。
    pub env: Vec<(String, String)>,
    /// 覆盖镜像 ENTRYPOINT(扫描器等自带 entrypoint 的镜像需置空,
    /// 否则 `sh -c` 会被当作子命令传入)。
    pub entrypoint: Option<Vec<String>>,
}

/// 输出截断上限。
const MAX_OUTPUT: usize = 64 * 1024;

fn truncate(s: String) -> String {
    if s.len() <= MAX_OUTPUT {
        return s;
    }
    let mut cut = MAX_OUTPUT;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…(截断)", &s[..cut])
}

/// Shell 后端:本机 `sh -c`(workdir 直接作为 cwd)。
#[derive(Default)]
pub struct ShellExecutor;

impl ShellExecutor {
    /// 执行。
    ///
    /// # Errors
    /// 进程启动失败(如 workdir 不存在)。
    pub async fn run_step(&self, spec: &StepSpec) -> Result<StepOutcome, AppError> {
        use tokio::io::AsyncReadExt as _;
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(&spec.command)
            .current_dir(&spec.workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;
        let mut output = Vec::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_end(&mut output).await;
        }
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_end(&mut output).await;
        }
        let status = child
            .wait()
            .await
            .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;
        Ok(StepOutcome {
            exit_code: status.code().unwrap_or(-1),
            output: truncate(String::from_utf8_lossy(&output).into_owned()),
        })
    }
}

/// Docker 后端(bollard):每步一个容器
/// (镜像内 `sh -c`;workdir bind-mount 到 /workspace;结束后删除容器)。
pub struct DockerExecutor {
    docker: bollard::Docker,
    /// runs-on 标签 → 镜像映射(如 ubuntu-latest → alpine:3)。
    image: String,
}

impl DockerExecutor {
    /// 连接本机 daemon:DOCKER_HOST 优先,其后 macOS Docker Desktop 与
    /// Linux 惯例路径逐个探测。
    ///
    /// # Errors
    /// 全部候选连接失败。
    pub fn connect(image: impl Into<String>) -> Result<Self, AppError> {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            std::env::var("DOCKER_HOST")
                .ok()
                .filter(|h| h.starts_with("unix://")),
            Some("unix:///var/run/docker.sock".to_owned()),
            Some(format!("unix://{home}/.docker/run/docker.sock")),
            Some("unix:///opt/homebrew/run/docker.sock".to_owned()),
        ];
        let mut last_err: Option<String> = None;
        for cand in candidates.into_iter().flatten() {
            let path = cand.trim_start_matches("unix://");
            if !std::path::Path::new(path).exists() {
                last_err = Some(format!("socket 不存在: {path}"));
                continue;
            }
            match bollard::Docker::connect_with_socket(path, 120, bollard::API_DEFAULT_VERSION) {
                Ok(docker) => {
                    return Ok(Self {
                        docker,
                        image: image.into(),
                    });
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        Err(
            AppError::internal("CI").with_details(serde_json::json!(format!(
                "Docker 连接失败: {}",
                last_err.unwrap_or_else(|| "无候选".to_owned())
            ))),
        )
    }

    /// daemon 可达性。
    ///
    /// # Errors
    /// ping 失败。
    pub async fn ping(&self) -> Result<(), AppError> {
        self.docker
            .ping()
            .await
            .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;
        Ok(())
    }

    /// 执行(容器名带随机后缀,防并发冲突;无论成败删除容器)。
    ///
    /// # Errors
    /// 容器创建/启动/等待失败。
    pub async fn run_step(&self, spec: &StepSpec) -> Result<StepOutcome, AppError> {
        use bollard::models::ContainerCreateBody;
        use bollard::query_parameters::{
            CreateContainerOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
            StartContainerOptionsBuilder, WaitContainerOptionsBuilder,
        };
        use futures_util::StreamExt as _;

        let name = format!("codeplan-step-{}", uuid::Uuid::new_v4().simple());
        let workdir_host = spec
            .workdir
            .to_str()
            .ok_or_else(|| AppError::invalid("CI", "workdir 非法"))?;
        let config = ContainerCreateBody {
            image: Some(self.image.clone()),
            cmd: Some(vec!["sh".to_owned(), "-c".to_owned(), spec.command.clone()]),
            working_dir: Some("/workspace".to_owned()),
            entrypoint: spec.entrypoint.clone(),
            host_config: Some(bollard::models::HostConfig {
                binds: Some(vec![format!("{workdir_host}:/workspace")]),
                ..Default::default()
            }),
            env: Some(spec.env.iter().map(|(k, v)| format!("{k}={v}")).collect()),
            ..Default::default()
        };
        self.docker
            .create_container(
                Some(CreateContainerOptionsBuilder::default().name(&name).build()),
                config,
            )
            .await
            .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;

        let result: Result<StepOutcome, AppError> = async {
            self.docker
                .start_container(&name, Some(StartContainerOptionsBuilder::default().build()))
                .await
                .map_err(|e| {
                    AppError::internal("CI").with_details(serde_json::json!(e.to_string()))
                })?;
            let status = self
                .docker
                .wait_container(&name, Some(WaitContainerOptionsBuilder::default().build()))
                .next()
                .await
                .ok_or_else(|| {
                    AppError::internal("CI").with_details(serde_json::json!("wait 流中断"))
                })?
                .map_err(|e| {
                    AppError::internal("CI").with_details(serde_json::json!(e.to_string()))
                })?;
            let exit_code = status.status_code;

            // 拉日志(stdout+stderr 合并)
            let mut logs = String::new();
            let mut stream = self.docker.logs(
                &name,
                Some(
                    LogsOptionsBuilder::default()
                        .stdout(true)
                        .stderr(true)
                        .tail("all")
                        .build(),
                ),
            );
            while let Some(chunk) = stream.next().await {
                if let Ok(
                    bollard::container::LogOutput::StdOut { message }
                    | bollard::container::LogOutput::StdErr { message }
                    | bollard::container::LogOutput::Console { message }
                    | bollard::container::LogOutput::StdIn { message },
                ) = chunk
                {
                    logs.push_str(&String::from_utf8_lossy(&message));
                }
            }
            Ok(StepOutcome {
                exit_code: exit_code as i32,
                output: truncate(logs),
            })
        }
        .await;

        let _ = self
            .docker
            .remove_container(
                &name,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cp-exec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn shell_echo_and_env() {
        let dir = temp_workdir();
        let ex = ShellExecutor;
        let out = ex
            .run_step(&StepSpec {
                command: "echo hello-ci && echo \"v=$VER\"".into(),
                workdir: dir.clone(),
                env: vec![("VER".into(), "9".into())],
                entrypoint: None,
            })
            .await
            .unwrap();
        assert!(out.success());
        assert!(out.output.contains("hello-ci"));
        assert!(out.output.contains("v=9"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn shell_failure_and_workdir() {
        let dir = temp_workdir();
        std::fs::write(dir.join("marker.txt"), "x").unwrap();
        let ex = ShellExecutor;
        let out = ex
            .run_step(&StepSpec {
                command: "cat marker.txt && exit 3".into(),
                workdir: dir.clone(),
                env: vec![],
                entrypoint: None,
            })
            .await
            .unwrap();
        assert_eq!(out.exit_code, 3);
        assert!(!out.success());
        assert!(out.output.contains("x"), "workdir 生效(读到文件)");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Docker 冒烟(需本机 daemon + alpine:3):`cargo test -p codeplan-ci -- --ignored`
    #[tokio::test]
    #[ignore = "需要 Docker daemon"]
    async fn docker_step_runs() {
        let dir = temp_workdir();
        std::fs::write(dir.join("hi.txt"), "docker-workdir").unwrap();
        let ex = DockerExecutor::connect("alpine:3").unwrap();
        ex.ping().await.unwrap();
        let out = ex
            .run_step(&StepSpec {
                command: "cat /workspace/hi.txt && echo container-ok".into(),
                workdir: dir.clone(),
                env: vec![],
                entrypoint: None,
            })
            .await
            .unwrap();
        assert!(out.success(), "output={}", out.output);
        assert!(out.output.contains("docker-workdir"));
        assert!(out.output.contains("container-ok"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
