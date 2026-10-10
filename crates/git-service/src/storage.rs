//! 裸仓库文件系统管理:`git init --bare` + 路径布局 `<root>/{full_name}.git`。

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::domain::BareRepoStore;

/// 文件系统裸仓库存储。
pub struct FsBareRepoStore {
    root: PathBuf,
}

impl FsBareRepoStore {
    /// 构造(根目录,如 `<data>/repositories`)。
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 仓库根目录。
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn validate_full_name(full_name: &str) -> Result<(), String> {
        let ok = full_name.contains('/')
            && full_name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '/')
            && !full_name.contains("..")
            && full_name.len() <= 255;
        if ok {
            Ok(())
        } else {
            Err(format!("非法仓库全名:{full_name}"))
        }
    }
}

#[async_trait]
impl BareRepoStore for FsBareRepoStore {
    async fn init_bare(&self, full_name: &str, default_branch: &str) -> Result<PathBuf, String> {
        Self::validate_full_name(full_name)?;
        let path = self.repo_path(full_name);
        if path.exists() {
            return Err(format!("仓库目录已存在:{}", path.display()));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        let out = Command::new("git")
            .args(["init", "--bare", "--initial-branch", default_branch])
            .arg(&path)
            .output()
            .await
            .map_err(|e| format!("启动 git 失败: {e}"))?;
        if !out.status.success() {
            // 兼容不支持 --initial-branch 的旧 git:退化为普通 bare 初始化
            let fallback = Command::new("git")
                .args(["init", "--bare"])
                .arg(&path)
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !fallback.status.success() {
                return Err(format!(
                    "git init 失败: {}{}",
                    String::from_utf8_lossy(&out.stderr),
                    String::from_utf8_lossy(&fallback.stderr)
                ));
            }
        }
        Ok(path)
    }

    fn repo_path(&self, full_name: &str) -> PathBuf {
        self.root.join(format!("{full_name}.git"))
    }
}
