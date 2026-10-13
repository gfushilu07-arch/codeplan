//! Runner 池与标签调度(T-302):注册/标签匹配/占用互斥;
//! job 级编排(逐步 fail-fast)在此层,执行原语见 executor。

use std::collections::HashMap;

use codeplan_common::error::AppError;
use tokio::sync::Mutex;

use crate::executor::{ShellExecutor, StepOutcome, StepSpec};

/// Runner 类型。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    /// 本机 shell。
    Shell,
    /// Docker 容器。
    Docker {
        /// 容器镜像(如 alpine:3)。
        image: String,
    },
}

/// 注册的 runner。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Runner {
    /// id(全局唯一;外部注册方自带或自动生成)。
    pub id: String,
    /// 标签(job runs-on 匹配;如 ubuntu-latest)。
    pub labels: Vec<String>,
    /// 类型。
    pub kind: RunnerKind,
    /// 是否忙(执行中)。
    pub busy: bool,
    /// 最近心跳。
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

/// 单步结果记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StepRecord {
    /// 步骤名。
    pub name: String,
    /// 成败。
    pub success: bool,
    /// 退出码。
    pub exit_code: i32,
    /// 输出尾部。
    pub output: String,
}

/// job 执行结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobOutcome {
    /// 成败(fail-fast:首个失败步终止)。
    pub success: bool,
    /// 各步记录。
    pub steps: Vec<StepRecord>,
}

/// Runner 池 + 执行编排。
pub struct RunnerPool {
    runners: Mutex<HashMap<String, Runner>>,
    shell: ShellExecutor,
}

impl RunnerPool {
    /// 空池。
    #[must_use]
    pub fn new() -> Self {
        Self {
            runners: Mutex::new(HashMap::new()),
            shell: ShellExecutor,
        }
    }

    /// 注册(同 id 覆盖标签/类型;busy 复位)。
    ///
    /// # Errors
    /// id 空。
    pub async fn register(
        &self,
        id: String,
        labels: Vec<String>,
        kind: RunnerKind,
    ) -> Result<Runner, AppError> {
        if id.trim().is_empty() {
            return Err(AppError::invalid("CI", "runner id 不能为空"));
        }
        let runner = Runner {
            busy: false,
            last_seen: chrono::Utc::now(),
            id: id.clone(),
            labels,
            kind,
        };
        self.runners.lock().await.insert(id, runner.clone());
        Ok(runner)
    }

    /// 心跳(刷新 last_seen;不存在则 Err)。
    ///
    /// # Errors
    /// 未注册。
    pub async fn heartbeat(&self, id: &str) -> Result<(), AppError> {
        let mut all = self.runners.lock().await;
        let r = all
            .get_mut(id)
            .ok_or_else(|| AppError::not_found("CI", "runner 未注册"))?;
        r.last_seen = chrono::Utc::now();
        Ok(())
    }

    /// 列表快照。
    pub async fn list(&self) -> Vec<Runner> {
        let mut all: Vec<Runner> = self.runners.lock().await.values().cloned().collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// 标签匹配 + 占用(原子);无空闲匹配返回 None。
    pub async fn acquire(&self, runs_on: &str) -> Option<String> {
        let mut all = self.runners.lock().await;
        let mut picked: Option<String> = None;
        for r in all.values() {
            if r.busy || !r.labels.iter().any(|l| l == runs_on) {
                continue;
            }
            match &picked {
                Some(id) if id < &r.id => {}
                _ => picked = Some(r.id.clone()),
            }
        }
        if let Some(id) = &picked
            && let Some(r) = all.get_mut(id)
        {
            r.busy = true;
        }
        picked
    }

    /// 释放。
    pub async fn release(&self, id: &str) {
        if let Some(r) = self.runners.lock().await.get_mut(id) {
            r.busy = false;
            r.last_seen = chrono::Utc::now();
        }
    }

    /// 在指定 runner 上执行 job(逐步 fail-fast;矩阵已在上层展开)。
    ///
    /// # Errors
    /// runner 不存在;执行器故障(workdir 缺失/容器错误)。
    pub async fn run_job(
        &self,
        runner_id: &str,
        workdir: &std::path::Path,
        steps: &[(String, String)],
        env: &[(String, String)],
    ) -> Result<JobOutcome, AppError> {
        let kind = {
            let all = self.runners.lock().await;
            let r = all
                .get(runner_id)
                .ok_or_else(|| AppError::not_found("CI", "runner 未注册"))?;
            r.kind.clone()
        };

        let mut records: Vec<StepRecord> = Vec::new();
        let mut success = true;
        for (name, command) in steps {
            let spec = StepSpec {
                command: command.clone(),
                workdir: workdir.to_path_buf(),
                env: env.to_vec(),
                entrypoint: None,
            };
            let outcome: StepOutcome = match &kind {
                RunnerKind::Shell => self.shell.run_step(&spec).await?,
                RunnerKind::Docker { image } => {
                    let ex = crate::executor::DockerExecutor::connect(image.clone())?;
                    ex.run_step(&spec).await?
                }
            };
            let ok = outcome.success();
            records.push(StepRecord {
                name: name.clone(),
                success: ok,
                exit_code: outcome.exit_code,
                output: outcome.output,
            });
            if !ok {
                success = false;
                break; // fail-fast
            }
        }
        Ok(JobOutcome {
            success,
            steps: records,
        })
    }
}

impl Default for RunnerPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cp-runner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn register_and_label_match() {
        let pool = RunnerPool::new();
        pool.register(
            "r-shell".into(),
            vec!["shell-only".into()],
            RunnerKind::Shell,
        )
        .await
        .unwrap();
        pool.register(
            "r-docker".into(),
            vec!["ubuntu-latest".into(), "gpu".into()],
            RunnerKind::Docker {
                image: "alpine:3".into(),
            },
        )
        .await
        .unwrap();

        // 标签匹配取 id 最小者;占用后互斥
        let first = pool.acquire("ubuntu-latest").await.unwrap();
        assert_eq!(first, "r-docker");
        assert!(
            pool.acquire("ubuntu-latest").await.is_none(),
            "占满后无匹配"
        );
        pool.release(&first).await;
        assert!(
            pool.acquire("ubuntu-latest").await.is_some(),
            "释放后可再占"
        );
        assert!(pool.acquire("shell-only").await.is_some(), "另一标签独立");

        // 无匹配标签
        assert!(pool.acquire("windows").await.is_none());

        pool.heartbeat("r-shell").await.unwrap();
        assert!(pool.heartbeat("ghost").await.is_err());
    }

    #[tokio::test]
    async fn run_job_fail_fast_on_shell_runner() {
        let pool = RunnerPool::new();
        pool.register("r1".into(), vec!["lab".into()], RunnerKind::Shell)
            .await
            .unwrap();
        let rid = pool.acquire("lab").await.unwrap();
        let d = dir();

        let outcome = pool
            .run_job(
                &rid,
                &d,
                &[
                    ("通过步".into(), "echo ok".into()),
                    ("失败步".into(), "echo bad >&2; exit 2".into()),
                    ("不应执行".into(), "echo never".into()),
                ],
                &[],
            )
            .await
            .unwrap();
        assert!(!outcome.success);
        assert_eq!(outcome.steps.len(), 2, "fail-fast 截断");
        assert!(outcome.steps[0].success);
        assert!(!outcome.steps[1].success);
        assert_eq!(outcome.steps[1].exit_code, 2);
        assert!(outcome.steps[1].output.contains("bad"));

        // 全部成功
        let ok = pool
            .run_job(&rid, &d, &[("唯一步".into(), "true".into())], &[])
            .await
            .unwrap();
        assert!(ok.success && ok.steps.len() == 1);
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn run_job_requires_runner() {
        let pool = RunnerPool::new();
        let d = dir();
        let err = pool
            .run_job("ghost", &d, &[("x".into(), "true".into())], &[])
            .await
            .unwrap_err();
        assert_eq!(err.code(), "CI-404");
        let _ = std::fs::remove_dir_all(d);
    }

    /// Docker runner 冒烟:`cargo test -p codeplan-ci -- --ignored`
    #[tokio::test]
    #[ignore = "需要 Docker daemon"]
    async fn run_job_on_docker_runner() {
        let pool = RunnerPool::new();
        pool.register(
            "d1".into(),
            vec!["ubuntu-latest".into()],
            RunnerKind::Docker {
                image: "alpine:3".into(),
            },
        )
        .await
        .unwrap();
        let rid = pool.acquire("ubuntu-latest").await.unwrap();
        let d = dir();
        std::fs::write(d.join("f.txt"), "from-host").unwrap();
        let outcome = pool
            .run_job(
                &rid,
                &d,
                &[("容器步".into(), "cat /workspace/f.txt && uname -m".into())],
                &[],
            )
            .await
            .unwrap();
        assert!(outcome.success, "{:?}", outcome.steps);
        assert!(outcome.steps[0].output.contains("from-host"));
        let _ = std::fs::remove_dir_all(d);
    }
}
