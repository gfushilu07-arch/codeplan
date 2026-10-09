//! 附件存储:本地文件系统(v1;存储抽象落 OpenDAL 后切换后端,接口不变)。

use std::path::{Path, PathBuf};

use codeplan_common::error::AppError;
use tokio::fs;

use crate::DOMAIN_CODE;

/// 文件系统附件存储。
pub struct FsAttachmentStore {
    root: PathBuf,
}

impl FsAttachmentStore {
    /// 构造(根目录,如 `<data>/attachments`)。
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    /// 保存字节流,key 由调用方生成(全局唯一)。
    ///
    /// # Errors
    /// 写盘失败。
    pub async fn save(&self, key: &str, bytes: &[u8]) -> Result<(), AppError> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        }
        fs::write(&path, bytes).await.map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })
    }

    /// 读取字节流。
    ///
    /// # Errors
    /// 文件不存在或读盘失败。
    pub async fn read(&self, key: &str) -> Result<Vec<u8>, AppError> {
        let path = self.path_for(key);
        if !path.exists() {
            return Err(AppError::not_found(DOMAIN_CODE, "附件文件缺失"));
        }
        fs::read(&path).await.map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })
    }

    /// 删除文件(不存在视为成功)。
    ///
    /// # Errors
    /// 删除失败(非不存在)。
    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        let path = self.path_for(key);
        if path.exists() {
            fs::remove_file(&path).await.map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        }
        Ok(())
    }

    /// 存储根目录。
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}
