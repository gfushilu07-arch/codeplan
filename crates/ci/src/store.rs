//! 流水线存储(T-303):pipeline/run/step 的 PG 实现。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::RepoId;
use sqlx::{PgPool, Row};

/// 域错误码。
pub const DC: &str = "CI";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

/// 已保存的 workflow。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Pipeline {
    /// ID。
    pub id: uuid::Uuid,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 名称。
    pub name: String,
    /// YAML 原文。
    pub yaml: String,
    /// 解析摘要(parsed workflow JSON)。
    pub parsed: serde_json::Value,
    /// 启用。
    pub enabled: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 运行记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Run {
    /// ID。
    pub id: uuid::Uuid,
    /// pipeline。
    pub pipeline_id: uuid::Uuid,
    /// 仓库。
    pub repo_id: RepoId,
    /// 分支。
    pub branch: String,
    /// 提交。
    pub sha: String,
    /// 状态:queued/running/success/failure。
    pub status: String,
    /// 触发:push/manual。
    pub trigger: String,
    /// 开始。
    pub started_at: Option<DateTime<Utc>>,
    /// 结束。
    pub finished_at: Option<DateTime<Utc>>,
}

/// 步骤记录。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunStep {
    /// job id。
    pub job_id: String,
    /// 矩阵组合标识。
    pub combo: String,
    /// 序号。
    pub seq: i32,
    /// 步骤名。
    pub name: String,
    /// 状态:pending/running/success/failure。
    pub status: String,
    /// 退出码。
    pub exit_code: Option<i32>,
    /// 输出尾部。
    pub output: String,
}

/// 存储。
pub struct CiStore {
    pool: PgPool,
}

impl CiStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 只读连接池(engine 内联查询用)。
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 保存 pipeline(upsert:repo+name 唯一)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn save_pipeline(
        &self,
        repo_id: RepoId,
        name: &str,
        yaml: &str,
        parsed: &serde_json::Value,
    ) -> Result<uuid::Uuid, AppError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO ci_pipeline (id, repo_id, name, yaml, parsed)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (repo_id, name) DO UPDATE SET
                yaml = $4, parsed = $5, updated_at = now()",
        )
        .bind(id)
        .bind(repo_id.as_uuid())
        .bind(name)
        .bind(yaml)
        .bind(parsed)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        // upsert 后取真实 id(冲突更新时生成 id 被丢弃)
        let row: (uuid::Uuid,) =
            sqlx::query_as("SELECT id FROM ci_pipeline WHERE repo_id = $1 AND name = $2")
                .bind(repo_id.as_uuid())
                .bind(name)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(row.0)
    }

    /// 仓库 pipeline 列表。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_pipelines(&self, repo_id: RepoId) -> Result<Vec<Pipeline>, AppError> {
        let rows = sqlx::query(
            "SELECT id, repo_id, name, yaml, parsed, enabled, created_at
             FROM ci_pipeline WHERE repo_id = $1 ORDER BY created_at",
        )
        .bind(repo_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| Pipeline {
                id: r.get("id"),
                repo_id: RepoId::from_uuid(r.get("repo_id")),
                name: r.get("name"),
                yaml: r.get("yaml"),
                parsed: r.get("parsed"),
                enabled: r.get("enabled"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// 按 id 取 pipeline。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn pipeline_by_id(&self, id: uuid::Uuid) -> Result<Option<Pipeline>, AppError> {
        let row = sqlx::query(
            "SELECT id, repo_id, name, yaml, parsed, enabled, created_at
             FROM ci_pipeline WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|r| Pipeline {
            id: r.get("id"),
            repo_id: RepoId::from_uuid(r.get("repo_id")),
            name: r.get("name"),
            yaml: r.get("yaml"),
            parsed: r.get("parsed"),
            enabled: r.get("enabled"),
            created_at: r.get("created_at"),
        }))
    }

    /// 删除 pipeline(级联 run)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn delete_pipeline(&self, id: uuid::Uuid) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM ci_pipeline WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "pipeline 不存在"));
        }
        Ok(())
    }

    /// 创建 run(queued)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn create_run(
        &self,
        pipeline_id: uuid::Uuid,
        repo_id: RepoId,
        branch: &str,
        sha: &str,
        trigger: &str,
        actor: Option<uuid::Uuid>,
    ) -> Result<uuid::Uuid, AppError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO ci_run (id, pipeline_id, repo_id, branch, sha, status, trigger_kind, actor)
             VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7)",
        )
        .bind(id)
        .bind(pipeline_id)
        .bind(repo_id.as_uuid())
        .bind(branch)
        .bind(sha)
        .bind(trigger)
        .bind(actor)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(id)
    }

    /// 更新 run 状态。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn set_run_status(&self, id: uuid::Uuid, status: &str) -> Result<(), AppError> {
        let finished = matches!(status, "success" | "failure");
        sqlx::query(
            "UPDATE ci_run SET status = $2,
                started_at = CASE WHEN $2 = 'running' THEN now() ELSE started_at END,
                finished_at = CASE WHEN $3 THEN now() ELSE finished_at END
             WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(finished)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// 追加步骤(存在则覆盖输出/状态)。
    ///
    /// # Errors
    /// 存储故障。
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_step(
        &self,
        run_id: uuid::Uuid,
        job_id: &str,
        combo: &str,
        seq: i32,
        name: &str,
        status: &str,
        exit_code: Option<i32>,
        output: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO ci_run_step
             (id, run_id, job_id, combo, seq, name, status, exit_code, output)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (run_id, job_id, combo, seq) DO UPDATE SET
                status = $7, exit_code = $8, output = $9",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(run_id)
        .bind(job_id)
        .bind(combo)
        .bind(seq)
        .bind(name)
        .bind(status)
        .bind(exit_code)
        .bind(output)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// run 详情。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn run_by_id(&self, id: uuid::Uuid) -> Result<Option<Run>, AppError> {
        let row = sqlx::query(
            "SELECT id, pipeline_id, repo_id, branch, sha, status, trigger_kind,
                    started_at, finished_at
             FROM ci_run WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(map_run))
    }

    /// 仓库 runs(倒序)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_runs(&self, repo_id: RepoId, limit: i64) -> Result<Vec<Run>, AppError> {
        let rows = sqlx::query(
            "SELECT id, pipeline_id, repo_id, branch, sha, status, trigger_kind,
                    started_at, finished_at
             FROM ci_run WHERE repo_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(repo_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(map_run).collect())
    }

    /// run 的步骤列表。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_steps(&self, run_id: uuid::Uuid) -> Result<Vec<RunStep>, AppError> {
        let rows = sqlx::query(
            "SELECT job_id, combo, seq, name, status, exit_code, output
             FROM ci_run_step WHERE run_id = $1 ORDER BY job_id, combo, seq",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| RunStep {
                job_id: r.get("job_id"),
                combo: r.get("combo"),
                seq: r.get("seq"),
                name: r.get("name"),
                status: r.get("status"),
                exit_code: r.get("exit_code"),
                output: r.get("output"),
            })
            .collect())
    }

    /// 仓库启用的 push 匹配 pipelines(引擎触发用)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn enabled_pipelines(&self, repo_id: RepoId) -> Result<Vec<Pipeline>, AppError> {
        Ok(self
            .list_pipelines(repo_id)
            .await?
            .into_iter()
            .filter(|p| p.enabled)
            .collect())
    }
}

fn map_run(r: sqlx::postgres::PgRow) -> Run {
    Run {
        id: r.get("id"),
        pipeline_id: r.get("pipeline_id"),
        repo_id: RepoId::from_uuid(r.get("repo_id")),
        branch: r.get("branch"),
        sha: r.get("sha"),
        status: r.get("status"),
        trigger: r.get("trigger_kind"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
    }
}
