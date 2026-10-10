//! Git SSH 服务(T-201):russh 服务端,公钥认证(指纹查目录)+
//! exec `git-upload-pack`/`git-receive-pack` + RBAC 校验;push 事件由
//! post-receive 钩子承接(与 HTTP 路径同一管道)。

use std::collections::HashMap;
use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authn::SshKeyDirectory;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::ids::{RepoId, UserId};
use russh::ChannelId;
use russh::keys::HashAlg;
use russh::server::Auth;
use tokio::sync::mpsc;

use crate::DOMAIN_CODE;
use crate::domain::{BareRepoStore, RepositoryRepository};

/// SSH Handler 错误包装(满足 `Error: From<russh::Error>`)。
#[derive(Debug)]
pub struct SshError(AppError);

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SshError {}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        Self(AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))
    }
}

impl From<AppError> for SshError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

/// 依赖集合。
#[derive(Clone)]
pub struct SshDeps {
    /// 指纹 → 用户(kernel trait,identity 实现)。
    pub directory: Arc<dyn SshKeyDirectory>,
    /// 仓库元数据。
    pub repos: Arc<dyn RepositoryRepository>,
    /// 权限。
    pub authz: Arc<dyn Authz>,
    /// 裸仓库路径。
    pub bare: Arc<dyn BareRepoStore>,
}

/// SSH 服务配置。
pub struct SshConfig {
    /// 监听地址(如 `0.0.0.0:2222`)。
    pub addr: String,
    /// 仓库根。
    pub repo_root: String,
}

/// 服务器顶层。
pub struct SshServer {
    /// 依赖。
    pub deps: SshDeps,
    /// 配置。
    pub config: SshConfig,
    /// 宿主密钥。
    pub host_keys: Vec<russh::keys::PrivateKey>,
}

/// 加载或生成宿主密钥(持久化于 `<data>/ssh_host_ed25519`)。
pub fn load_or_generate_host_key(
    path: &std::path::Path,
) -> Result<russh::keys::PrivateKey, String> {
    if path.exists() {
        return russh::keys::PrivateKey::read_openssh_file(path)
            .map_err(|e| format!("读取宿主密钥失败: {e}"));
    }
    // v1:宿主密钥生成走 ssh-keygen CLI(与读路径 CLI 策略一致;后续可切 russh 生成)
    let out = std::process::Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-q", "-f"])
        .arg(path)
        .output()
        .map_err(|e| format!("启动 ssh-keygen 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ssh-keygen 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    russh::keys::PrivateKey::read_openssh_file(path).map_err(|e| format!("读取宿主密钥失败: {e}"))
}

impl russh::server::Server for SshServer {
    type Handler = SshHandler;

    fn new_client(&mut self, _addr: Option<std::net::SocketAddr>) -> Self::Handler {
        SshHandler {
            deps: self.deps.clone(),
            user: None,
            senders: HashMap::new(),
        }
    }
}

/// 每连接 Handler。
pub struct SshHandler {
    deps: SshDeps,
    user: Option<(UserId, String)>,
    senders: HashMap<ChannelId, mpsc::Sender<Vec<u8>>>,
}

impl SshHandler {
    async fn ensure(
        &self,
        user: &UserId,
        action: Action,
        repo: &crate::domain::Repository,
    ) -> Result<(), SshError> {
        let reference = ResourceRef::Repo(RepoId::from_uuid(repo.id.as_uuid()));
        self.deps
            .authz
            .check(*user, action, &reference)
            .await
            .map_err(|e| {
                SshError(AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))
            })?
            .then_some(())
            .ok_or_else(|| SshError(AppError::forbidden(DOMAIN_CODE, "无权限执行该操作")))
    }

    fn full_name_from_command_path(&self, raw: &str) -> Option<String> {
        let trimmed = raw.trim_matches('\'').trim_matches('"');
        let stripped = trimmed.strip_prefix('/')?.strip_suffix(".git")?;
        let mut segs = stripped.split('/');
        let owner = segs.next()?;
        let key = segs.next()?;
        if segs.next().is_some() || owner.is_empty() || key.is_empty() {
            return None;
        }
        Some(format!("{owner}/{key}"))
    }

    fn repo_path(&self, full_name: &str) -> std::path::PathBuf {
        self.deps.bare.repo_path(full_name)
    }
}

impl russh::server::Handler for SshHandler {
    type Error = SshError;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        public_key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();
        let found = self
            .deps
            .directory
            .user_by_fingerprint(&fingerprint)
            .await
            .map_err(|e| {
                SshError(AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))
            })?;
        self.user = found.map(|u| (u.user_id, u.username));
        Ok(if self.user.is_some() {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::reject())
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let _ = channel; // exec_request 经 channel_id 关联
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let Some(user) = self.user.clone() else {
            let _ = session.channel_failure(channel);
            return Ok(());
        };
        let command = String::from_utf8_lossy(command).to_string();
        eprintln!("[ssh] exec: {command}");
        let (service, path_raw) = command.split_once(' ').unwrap_or(("", ""));
        let service = service.trim();
        let Some(full_name) = self.full_name_from_command_path(path_raw) else {
            let _ = session.channel_failure(channel);
            return Ok(());
        };
        let repo = self
            .deps
            .repos
            .find_by_full_name(&full_name)
            .await
            .map_err(SshError::from)?
            .ok_or_else(|| SshError(AppError::not_found(DOMAIN_CODE, "仓库不存在")))?;
        if repo.archived {
            let _ = session.channel_failure(channel);
            return Ok(());
        }
        let action = match service {
            "git-receive-pack" => Action::Write,
            "git-upload-pack" | "git-upload-archive" => Action::Read,
            _ => {
                let _ = session.channel_failure(channel);
                return Ok(());
            }
        };
        // 客户端命令形如 git-upload-pack,git 子命令需去前缀
        let Some(subcommand) = service.strip_prefix("git-") else {
            let _ = session.channel_failure(channel);
            return Ok(());
        };
        let (uid, _uname) = user;
        self.ensure(&uid, action, &repo).await?;

        let path = self.repo_path(&full_name);
        let mut child = tokio::process::Command::new("git")
            .arg(subcommand)
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                SshError(
                    AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())),
                )
            })?;

        let _ = session.channel_success(channel);

        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        // stdout → channel(EOF 即停止;通道由 watcher 在子进程退出后关闭)
        if let Some(mut stdout) = stdout.take() {
            let handle = session.handle();
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt as _;
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) | Err(_) => {
                            eprintln!("[ssh] stdout EOF(channel {channel:?})");
                            break;
                        }
                        Ok(n) => {
                            eprintln!("[ssh] 转发 {n} 字节");
                            if handle.data(channel, buf[..n].to_vec()).await.is_err() {
                                eprintln!("[ssh] data 写回失败(channel {channel:?})");
                                break;
                            }
                        }
                    }
                }
                let _ = handle.eof(channel).await;
            });
        }

        // client → git stdin:数据回调经 mpsc 桥接
        if let Some(mut stdin) = stdin.take() {
            let (tx, mut rx) = mpsc::channel::<Vec<u8>>(32);
            self.senders.insert(channel, tx);
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt as _;
                while let Some(chunk) = rx.recv().await {
                    if stdin.write_all(&chunk).await.is_err() {
                        break;
                    }
                }
                let _ = stdin.shutdown().await;
            });
        }

        // stderr → 服务日志
        if let Some(mut stderr) = stderr.take() {
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt as _;
                let mut buf = vec![0u8; 4096];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => eprintln!("[git-stderr] {}", String::from_utf8_lossy(&buf[..n])),
                    }
                }
            });
        }

        // 子进程退出 watcher(仅记录日志;通道由客户端关闭,避免过早截断数据)
        tokio::spawn(async move {
            let status = child.wait().await;
            eprintln!("[ssh] 子进程退出({status:?})");
        });
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if let Some(tx) = self.senders.get(&channel) {
            let _ = tx.send(data.to_vec()).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.senders.remove(&channel);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.senders.remove(&channel);
        Ok(())
    }
}
