//! 制品库存储层(T-304):元数据 PG + 内容 OpenDAL(`artifacts/`)。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::ids::RepoId;
use sqlx::PgPool;

use codeplan_storage::ObjectStore;

/// 域错误码。
pub const DC: &str = "REG";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

/// 制品元数据。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Artifact {
    /// ID。
    pub id: uuid::Uuid,
    /// 仓库。
    pub repo_id: RepoId,
    /// 协议种类。
    pub kind: String,
    /// 包名。
    pub name: String,
    /// 版本。
    pub version: String,
    /// 文件名。
    pub filename: String,
    /// 字节。
    pub size: i64,
    /// sha256。
    pub sha256: String,
    /// 附加元数据。
    pub meta: serde_json::Value,
}

/// 存储门面。
pub struct ArtifactStore {
    pool: PgPool,
    objects: Arc<ObjectStore>,
}

impl ArtifactStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool, objects: Arc<ObjectStore>) -> Self {
        Self { pool, objects }
    }

    /// 全名 → 仓库 ID。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn repo_id_by_full_name(&self, full_name: &str) -> Result<Option<RepoId>, AppError> {
        let row: Option<(uuid::Uuid,)> =
            sqlx::query_as("SELECT id FROM repo_repository WHERE full_name = $1")
                .bind(full_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(row.map(|(id,)| RepoId::from_uuid(id)))
    }

    /// 仓库内 kind 全列。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_by_kind(
        &self,
        repo_id: RepoId,
        kind: &str,
    ) -> Result<Vec<Artifact>, AppError> {
        let _ = repo_id; // 查询已按 repo_id 过滤(参数保留以对齐 API 语义)
        let rows: Vec<(
            uuid::Uuid,
            String,
            String,
            String,
            i64,
            String,
            serde_json::Value,
        )> = sqlx::query_as(
            "SELECT id, name, version, filename, size, sha256, meta FROM reg_artifact
                 WHERE repo_id = $1 AND kind = $2 ORDER BY name, version, created_at",
        )
        .bind(repo_id.as_uuid())
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, name, version, filename, size, sha256, meta)| Artifact {
                    id,
                    repo_id,
                    kind: kind.to_owned(),
                    name,
                    version,
                    filename,
                    size,
                    sha256,
                    meta,
                },
            )
            .collect())
    }

    fn object_key(kind: &str, name: &str, version: &str, filename: &str) -> String {
        let safe_name = name.replace(['/', ':'], "_");
        format!("artifacts/{kind}/{safe_name}/{version}/{filename}")
    }

    /// 上传(内容 + 元数据 upsert;同键覆盖)。
    ///
    /// # Errors
    /// 存储故障。
    #[allow(clippy::too_many_arguments)]
    pub async fn put(
        &self,
        repo_id: RepoId,
        kind: &str,
        name: &str,
        version: &str,
        filename: &str,
        data: Vec<u8>,
        meta: serde_json::Value,
        created_by: Option<uuid::Uuid>,
    ) -> Result<Artifact, AppError> {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&data);
        let sha256: String = hasher.finalize().iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
        let size = data.len() as i64;
        self.objects
            .put(&Self::object_key(kind, name, version, filename), data)
            .await
            .map_err(|e| AppError::internal(DC).with_details(serde_json::json!(e.to_string())))?;
        let row = sqlx::query(
            "INSERT INTO reg_artifact
             (id, repo_id, kind, name, version, filename, size, sha256, meta, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (repo_id, kind, name, version, filename) DO UPDATE SET
                size = $7, sha256 = $8, meta = $9
             RETURNING id",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(repo_id.as_uuid())
        .bind(kind)
        .bind(name)
        .bind(version)
        .bind(filename)
        .bind(size)
        .bind(&sha256)
        .bind(&meta)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(Artifact {
            id: sqlx::Row::get(&row, "id"),
            repo_id,
            kind: kind.to_owned(),
            name: name.to_owned(),
            version: version.to_owned(),
            filename: filename.to_owned(),
            size,
            sha256,
            meta,
        })
    }

    /// 名下全部文件。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn versions(
        &self,
        repo_id: RepoId,
        kind: &str,
        name: &str,
    ) -> Result<Vec<Artifact>, AppError> {
        let rows: Vec<(uuid::Uuid, String, String, i64, String, serde_json::Value)> =
            sqlx::query_as(
                "SELECT id, version, filename, size, sha256, meta FROM reg_artifact
                 WHERE repo_id = $1 AND kind = $2 AND name = $3 ORDER BY created_at",
            )
            .bind(repo_id.as_uuid())
            .bind(kind)
            .bind(name)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|(id, version, filename, size, sha256, meta)| Artifact {
                id,
                repo_id,
                kind: kind.to_owned(),
                name: name.to_owned(),
                version,
                filename,
                size,
                sha256,
                meta,
            })
            .collect())
    }

    /// 下载内容。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn content(
        &self,
        _repo_id: RepoId,
        kind: &str,
        name: &str,
        version: &str,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        self.objects
            .get(&Self::object_key(kind, name, version, filename))
            .await
            .map_err(|e| AppError::internal(DC).with_details(serde_json::json!(e.to_string())))
    }
}
