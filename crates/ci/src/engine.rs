//! 执行引擎(T-303):排队 → 领 runner → worktree 检出 → 逐步执行
//! (矩阵展开)→ 状态机落库 → SSE 广播 → 发布 `ci.run.finished` 事件
//! (code-review 消费写 commit status,闭环 PR 门禁)。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::events::{EventBus, EventEnvelope};
use codeplan_kernel::ids::RepoId;
use serde_json::json;
use tokio::sync::{Mutex, broadcast};

use crate::runner::RunnerPool;
use crate::store::CiStore;
use crate::workflow::{expand_matrix, parse_workflow, render_matrix_placeholders};

/// SSE 事件(run 粒度增量)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunEvent {
    /// run ID。
    pub run_id: uuid::Uuid,
    /// 类型:queued/step_started/step_finished/run_finished。
    pub kind: String,
    /// job id。
    pub job: String,
    /// 矩阵组合。
    pub combo: String,
    /// 步骤名。
    pub step: String,
    /// 载荷(输出片段/最终状态)。
    pub payload: serde_json::Value,
}

/// 引擎。
pub struct Engine {
    /// 存储。
    pub store: Arc<CiStore>,
    /// runner 池。
    pub runners: Arc<RunnerPool>,
    /// 裸仓库根(worktree 检出)。
    pub repo_root: String,
    /// 缓存目录根(Docker 挂载 / shell 复用)。
    pub cache_root: PathBuf,
    /// 事件总线(ci.run.finished)。
    pub events: Arc<dyn EventBus>,
    /// 制品库(流水线产物归档,可选)。
    pub artifacts: Option<Arc<crate::artifact_store::ArtifactStore>>,
    /// SSE 订阅。
    pub sse: broadcast::Sender<RunEvent>,
    /// 队列。
    queue: Mutex<VecDeque<uuid::Uuid>>,
}

impl Engine {
    /// 构造(并启动后台 worker)。
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        store: Arc<CiStore>,
        runners: Arc<RunnerPool>,
        repo_root: String,
        cache_root: PathBuf,
        events: Arc<dyn EventBus>,
        artifacts: Option<Arc<crate::artifact_store::ArtifactStore>>,
    ) -> Arc<Self> {
        let (sse, _) = broadcast::channel(256);
        let engine = Arc::new(Self {
            store,
            runners,
            repo_root,
            cache_root,
            events,
            artifacts,
            sse,
            queue: Mutex::new(VecDeque::new()),
        });
        // 单 worker 串行执行(v1;并发度 T-307 再扩)
        let worker = Arc::clone(&engine);
        tokio::spawn(async move {
            loop {
                let next = worker.queue.lock().await.pop_front();
                match next {
                    Some(run_id) => {
                        if let Err(e) = worker.execute_run(run_id).await {
                            eprintln!(
                                "[ci-engine] run {run_id} 执行失败: {} details={:?}",
                                e.message(),
                                e.details()
                            );
                            let _ = worker.store.set_run_status(run_id, "failure").await;
                            worker.broadcast(RunEvent {
                                run_id,
                                kind: "run_finished".into(),
                                job: String::new(),
                                combo: String::new(),
                                step: String::new(),
                                payload: serde_json::json!({
                                    "status": "failure",
                                    "reason": e.message(),
                                }),
                            });
                        }
                    }
                    None => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
                }
            }
        });
        engine
    }

    /// 入队。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn enqueue(&self, run_id: uuid::Uuid) -> Result<(), AppError> {
        self.queue.lock().await.push_back(run_id);
        let run = self
            .store
            .run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::not_found("CI", "run 不存在"))?;
        self.broadcast(RunEvent {
            run_id,
            kind: "queued".into(),
            job: String::new(),
            combo: String::new(),
            step: String::new(),
            payload: json!({"status": "queued"}),
        });
        let _ = run;
        Ok(())
    }

    fn broadcast(&self, event: RunEvent) {
        let _ = self.sse.send(event);
    }

    /// 执行一个 run(状态机:queued → running → success/failure)。
    ///
    /// # Errors
    /// run/pipeline 不存在;worktree 检出失败。
    #[allow(clippy::too_many_lines)]
    pub async fn execute_run(&self, run_id: uuid::Uuid) -> Result<(), AppError> {
        let run = self
            .store
            .run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::not_found("CI", "run 不存在"))?;
        let pipeline = self
            .store
            .pipeline_by_id(run.pipeline_id)
            .await?
            .ok_or_else(|| AppError::not_found("CI", "pipeline 不存在"))?;
        let wf = parse_workflow(&pipeline.yaml)
            .map_err(|e| AppError::invalid("CI", format!("workflow 非法: {e}")))?;

        self.store.set_run_status(run_id, "running").await?;

        // worktree 检出(sha 优先,缺省分支)
        let workdir = PathBuf::from(format!(
            "{}/.worktrees/run-{}",
            self.cache_root.display(),
            run_id.simple()
        ));
        let _ = std::fs::create_dir_all(&workdir);
        let repo_dir = format!(
            "{}/{}.git",
            self.repo_root,
            repo_full_name_by_id(&self.store, run.repo_id).await?
        );
        let reference = if run.sha.is_empty() {
            run.branch.clone()
        } else {
            run.sha.clone()
        };
        let out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&repo_dir)
            .args(["worktree", "add", "--detach"])
            .arg(&workdir)
            .arg(&reference)
            .output()
            .await
            .map_err(|e| AppError::internal("CI").with_details(json!(e.to_string())))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let _ = tokio::process::Command::new("git")
                .arg("--git-dir")
                .arg(&repo_dir)
                .args(["worktree", "prune"])
                .output()
                .await;
            self.store.set_run_status(run_id, "failure").await?;
            self.broadcast(RunEvent {
                run_id,
                kind: "run_finished".into(),
                job: String::new(),
                combo: String::new(),
                step: String::new(),
                payload: json!({"status": "failure", "reason": format!("checkout 失败: {msg}")}),
            });
            return Err(AppError::invalid("CI", format!("checkout 失败: {msg}")));
        }

        // 实际检出 SHA(手动 dispatch 可能只给分支)
        let resolved_sha = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&repo_dir)
            .args(["rev-parse", "HEAD"])
            .current_dir(&workdir)
            .output()
            .await
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| run.sha.clone());

        // 缓存目录(构建缓存卷:宿主持久目录,shell 直接复用;docker 由上层挂载)
        let repo_key = repo_full_name_by_id(&self.store, run.repo_id).await?;
        let cache_dir = self
            .cache_root
            .join("cache")
            .join(repo_key.replace('/', "_"));
        let _ = std::fs::create_dir_all(&cache_dir);

        let mut overall = true;
        for job in &wf.jobs {
            let env_base: Vec<(String, String)> = vec![
                ("CI".to_owned(), "true".to_owned()),
                ("CODEPLAN".to_owned(), "true".to_owned()),
                ("CODEPLAN_SHA".to_owned(), resolved_sha.clone()),
                ("CODEPLAN_BRANCH".to_owned(), run.branch.clone()),
            ];
            for (values, combo_suffix) in expand_matrix(job) {
                let combo_label = combo_suffix.trim_start_matches('-').to_owned();
                for (i, step) in job.steps.iter().enumerate() {
                    let command = match (&step.uses, &step.run) {
                        (_, Some(run)) => render_matrix_placeholders(run, &values),
                        (Some(u), None) => format!("echo [uses] {u}"),
                        _ => continue,
                    };
                    let step_name = render_matrix_placeholders(&step.name, &values);
                    self.store
                        .upsert_step(
                            run_id,
                            &job.id,
                            &combo_label,
                            i as i32,
                            &step_name,
                            "running",
                            None,
                            "",
                        )
                        .await?;
                    self.broadcast(RunEvent {
                        run_id,
                        kind: "step_started".into(),
                        job: job.id.clone(),
                        combo: combo_label.clone(),
                        step: step_name.clone(),
                        payload: json!({}),
                    });

                    // 步骤命令带缓存目录环境
                    let mut env = env_base.clone();
                    env.push(("CACHE_DIR".to_owned(), cache_dir.display().to_string()));
                    env.extend(step.env.clone());

                    let runner_id = self.wait_runner(&job.runs_on).await.ok_or_else(|| {
                        AppError::internal("CI").with_details(json!("无可用 runner"))
                    })?;
                    let outcome = self
                        .runners
                        .run_job(&runner_id, &workdir, &[(step_name.clone(), command)], &env)
                        .await;
                    self.runners.release(&runner_id).await;

                    match outcome {
                        Ok(job_outcome) => {
                            let rec = &job_outcome.steps[0];
                            self.store
                                .upsert_step(
                                    run_id,
                                    &job.id,
                                    &combo_label,
                                    i as i32,
                                    &step_name,
                                    if rec.success { "success" } else { "failure" },
                                    Some(rec.exit_code),
                                    &rec.output,
                                )
                                .await?;
                            self.broadcast(RunEvent {
                                run_id,
                                kind: "step_finished".into(),
                                job: job.id.clone(),
                                combo: combo_label.clone(),
                                step: step_name.clone(),
                                payload: json!({"success": rec.success, "output": rec.output, "exit_code": rec.exit_code}),
                            });
                            if !rec.success {
                                overall = false;
                                break;
                            }
                        }
                        Err(e) => {
                            self.store
                                .upsert_step(
                                    run_id,
                                    &job.id,
                                    &combo_label,
                                    i as i32,
                                    &step_name,
                                    "failure",
                                    Some(-1),
                                    e.message(),
                                )
                                .await?;
                            self.broadcast(RunEvent {
                                run_id,
                                kind: "step_finished".into(),
                                job: job.id.clone(),
                                combo: combo_label.clone(),
                                step: step_name.clone(),
                                payload: json!({"success": false, "output": e.message()}),
                            });
                            overall = false;
                            break;
                        }
                    }
                }
                if !overall {
                    break;
                }
            }
            if !overall {
                break;
            }
        }

        // 流水线产物打通(T-304):扫描 workdir/dist/* 归档到制品库
        if let Some(artifacts) = &self.artifacts
            && let Ok(mut rd) = tokio::fs::read_dir(workdir.join("dist")).await
        {
            while let Ok(Some(entry)) = rd.next_entry().await {
                if !entry
                    .file_type()
                    .await
                    .map(|t| t.is_file())
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Ok(data) = tokio::fs::read(entry.path()).await {
                    let filename = entry.file_name().to_string_lossy().into_owned();
                    let version = if resolved_sha.len() >= 8 {
                        resolved_sha[..8].to_owned()
                    } else {
                        resolved_sha.clone()
                    };
                    let _ = artifacts
                        .put(
                            run.repo_id,
                            "pipeline",
                            "dist",
                            &version,
                            &filename,
                            data,
                            json!({"run_id": run_id}),
                            None,
                        )
                        .await;
                }
            }
        }

        // worktree 清理
        let _ = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&repo_dir)
            .args(["worktree", "remove", "--force"])
            .arg(&workdir)
            .output()
            .await;

        let status = if overall { "success" } else { "failure" };
        self.store.set_run_status(run_id, status).await?;
        self.broadcast(RunEvent {
            run_id,
            kind: "run_finished".into(),
            job: String::new(),
            combo: String::new(),
            step: String::new(),
            payload: json!({"status": status}),
        });
        // 事件:code-review 消费写 commit status(context = ci/<pipeline 名>)
        let publish = self.events.publish(vec![EventEnvelope::new(
            "ci.run.finished",
            json!({
                "run_id": run_id,
                "repo_id": run.repo_id.as_uuid(),
                "full_name": repo_key,
                "sha": run.sha,
                "branch": run.branch,
                "context": format!("ci/{}", pipeline.name),
                "state": status,
                "pipeline": pipeline.name,
            }),
        )]);
        publish.await.ok();
        if !overall {
            return Err(AppError::conflict("CI", "流水线执行失败"));
        }
        Ok(())
    }

    /// 等 runner(最多 15s;无 → None)。
    async fn wait_runner(&self, runs_on: &str) -> Option<String> {
        for _ in 0..30 {
            if let Some(id) = self.runners.acquire(runs_on).await {
                return Some(id);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        None
    }
}

async fn repo_full_name_by_id(store: &CiStore, repo_id: RepoId) -> Result<String, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT full_name FROM repo_repository WHERE id = $1")
            .bind(repo_id.as_uuid())
            .fetch_optional(store.pool())
            .await
            .map_err(|e| AppError::internal("CI").with_details(json!(e.to_string())))?;
    row.map(|r| r.0)
        .ok_or_else(|| AppError::not_found("CI", "仓库不存在"))
}
