//! 裸仓库文件系统管理:`git init --bare` + 路径布局 `<root>/{full_name}.git`。

use async_trait::async_trait;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use std::fs::Permissions;
use tokio::fs;
use tokio::process::Command;

use crate::domain::BareRepoStore;

/// post-receive 钩子配置(平台回调地址 + 共享密钥)。
#[derive(Debug, Clone)]
pub struct HookConfig {
    /// 平台回调根地址(如 `http://localhost:8080`)。
    pub url: String,
    /// 钩子共享密钥(X-Hook-Secret 头)。
    pub secret: String,
}

/// 文件系统裸仓库存储。
pub struct FsBareRepoStore {
    root: PathBuf,
    hook: Option<HookConfig>,
}

impl FsBareRepoStore {
    /// 构造(根目录,如 `<data>/repositories`)。
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            hook: None,
        }
    }

    /// 配置 post-receive 钩子(init 时写入仓库)。
    #[must_use]
    pub fn with_hook(mut self, hook: HookConfig) -> Self {
        self.hook = Some(hook);
        self
    }

    /// 安装 post-receive 钩子:把 stdin 原样 POST 回平台内部端点。
    async fn install_post_receive_hook(&self, repo_path: &Path) -> Result<(), String> {
        let Some(hook) = &self.hook else {
            return Ok(());
        };
        let hooks_dir = repo_path.join("hooks");
        tokio::fs::create_dir_all(&hooks_dir)
            .await
            .map_err(|e| e.to_string())?;
        let script = format!(
            "#!/bin/sh\n# codeplan post-receive(自动生成)\nINPUT=$(cat)\ncurl -s -X POST \\
             '{url}/internal/hooks/post-receive' \\
             -H 'X-Hook-Secret: {secret}' \\
             -H \"X-Hook-Repo: $GIT_DIR\" \\
             -H 'Content-Type: text/plain' \\
             --data-binary \"$INPUT\" >/dev/null\n",
            url = hook.url,
            secret = hook.secret,
        );
        let hook_path = hooks_dir.join("post-receive");
        fs::write(&hook_path, script)
            .await
            .map_err(|e| e.to_string())?;
        fs::set_permissions(&hook_path, Permissions::from_mode(0o755))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
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
        self.install_post_receive_hook(&path).await?;
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
