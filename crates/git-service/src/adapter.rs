//! kernel Git 契约的 CLI 实现(T-203):评审域经 trait 使用,
//! 与 browse.rs 同一"git 官方 CLI porcelain"读路径铁律。
//!
//! 合并执行(服务端无工作区优先):
//! - merge:`git merge-tree --write-tree` + `git commit-tree`(双亲);
//! - squash:`git rev-parse H^{tree}` + 单亲 `commit-tree`;
//! - rebase:临时 worktree 中 `git cherry-pick M..H` 重放。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::DateTime;
use codeplan_kernel::git::{
    CommitBrief, MergeMethod, MergeOutcome, MergeSpec, RepoRef, RepoRefId, TagBrief,
};
use tokio::process::Command;

use crate::domain::RepositoryRepository;

/// CLI 实现:仓库根 + 仓库元数据(供 GitRepoLookup 投影)。
pub struct CliGitAccess {
    repo_root: String,
    repos: Arc<dyn RepositoryRepository>,
}

impl CliGitAccess {
    /// 构造(仓库根,与裸仓库存储一致)。
    #[must_use]
    pub fn new(repo_root: impl Into<String>, repos: Arc<dyn RepositoryRepository>) -> Self {
        Self {
            repo_root: repo_root.into(),
            repos,
        }
    }

    fn repo_path(&self, full_name: &str) -> PathBuf {
        PathBuf::from(&self.repo_root).join(format!("{full_name}.git"))
    }

    async fn git(&self, full_name: &str, args: &[&str]) -> Result<String, String> {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git {:?} 失败: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// 输出可能为空/失败的 git 查询(返回 None 而非 Err)。
    async fn git_opt(&self, full_name: &str, args: &[&str]) -> Result<Option<String>, String> {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        if !out.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        Ok(if text.is_empty() { None } else { Some(text) })
    }

    async fn commit_env(
        &self,
        full_name: &str,
        spec: &MergeSpec,
        args: Vec<String>,
    ) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(&args)
            .env("GIT_AUTHOR_NAME", &spec.committer_name)
            .env("GIT_AUTHOR_EMAIL", &spec.committer_email)
            .env("GIT_COMMITTER_NAME", &spec.committer_name)
            .env("GIT_COMMITTER_EMAIL", &spec.committer_email)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let message = if spec.message_body.trim().is_empty() {
            spec.message_title.clone()
        } else {
            format!("{}\n\n{}", spec.message_title, spec.message_body)
        };
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("git commit-tree 启动失败: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt as _;
            stdin
                .write_all(message.as_bytes())
                .await
                .map_err(|e| format!("写提交信息失败: {e}"))?;
            stdin.shutdown().await.ok();
        }
        let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "commit-tree 失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// 目标分支前移(带旧值 CAS,防并发合并竞态)。
    async fn advance_target(
        &self,
        full_name: &str,
        target_branch: &str,
        old: &str,
        new: &str,
    ) -> Result<(), String> {
        self.git(
            full_name,
            &[
                "update-ref",
                &format!("refs/heads/{target_branch}"),
                new,
                old,
            ],
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("目标分支已被并发更新: {e}"))
    }
}

#[async_trait]
impl codeplan_kernel::git::GitRepoLookup for CliGitAccess {
    async fn by_full_name(&self, full_name: &str) -> Result<Option<RepoRef>, String> {
        let repo = self
            .repos
            .find_by_full_name(full_name)
            .await
            .map_err(|e| e.to_string())?;
        Ok(repo.map(|r| RepoRef {
            id: RepoRefId::from_uuid(r.id.as_uuid()),
            full_name: r.full_name,
            default_branch: r.default_branch,
            archived: r.archived,
        }))
    }
}

#[async_trait]
impl codeplan_kernel::git::GitAccess for CliGitAccess {
    async fn resolve_ref(
        &self,
        full_name: &str,
        reference: &str,
    ) -> Result<Option<String>, String> {
        self.git_opt(
            full_name,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{commit}}"),
            ],
        )
        .await
    }

    async fn merge_base(
        &self,
        full_name: &str,
        a: &str,
        b: &str,
    ) -> Result<Option<String>, String> {
        self.git_opt(full_name, &["merge-base", a, b]).await
    }

    async fn diff_patch(&self, full_name: &str, base: &str, head: &str) -> Result<String, String> {
        let Some(base_commit) = self.resolve_ref(full_name, base).await? else {
            return Err(format!("基准 {base} 不存在"));
        };
        let Some(head_commit) = self.resolve_ref(full_name, head).await? else {
            return Err(format!("源 {head} 不存在"));
        };
        let Some(mb) = self
            .merge_base(full_name, &base_commit, &head_commit)
            .await?
        else {
            return Err("两分支无共同祖先".to_owned());
        };
        self.git(
            full_name,
            &["diff", "--patch", "--no-color", &mb, &head_commit],
        )
        .await
    }

    async fn commits_between(
        &self,
        full_name: &str,
        base: &str,
        head: &str,
    ) -> Result<Vec<CommitBrief>, String> {
        let Some(head_commit) = self.resolve_ref(full_name, head).await? else {
            return Err(format!("源 {head} 不存在"));
        };
        // base 为空 = 全量历史(上限 100);否则取 merge-base 区间
        let (range, reverse) = if base.trim().is_empty() {
            (head_commit.clone(), false)
        } else {
            let Some(base_commit) = self.resolve_ref(full_name, base).await? else {
                return Err(format!("基准 {base} 不存在"));
            };
            let Some(mb) = self
                .merge_base(full_name, &base_commit, &head_commit)
                .await?
            else {
                return Err("两分支无共同祖先".to_owned());
            };
            (format!("{mb}..{head_commit}"), true)
        };
        let mut args: Vec<String> = vec![
            "--no-pager".into(),
            "log".into(),
            "--max-count".into(),
            "100".into(),
            "--format=%H\x1f%an\x1f%ae\x1f%s\x1f%aI".into(),
        ];
        if reverse {
            args.insert(2, "--reverse".into());
        }
        args.push(range);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = self.git(full_name, &arg_refs).await?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| {
                let mut it = l.split('\x1f');
                Some(CommitBrief {
                    id: it.next()?.to_owned(),
                    author_name: it.next()?.to_owned(),
                    author_email: it.next()?.to_owned(),
                    message_title: it.next()?.to_owned(),
                    committed_at: DateTime::parse_from_rfc3339(it.next()?)
                        .ok()?
                        .with_timezone(&chrono::Utc),
                })
            })
            .collect())
    }

    async fn merge_branch(
        &self,
        full_name: &str,
        spec: &MergeSpec,
    ) -> Result<MergeOutcome, String> {
        let Some(target) = self.resolve_ref(full_name, &spec.target_branch).await? else {
            return Err(format!("目标分支 {} 不存在", spec.target_branch));
        };
        let Some(head) = self.resolve_ref(full_name, &spec.head_branch).await? else {
            return Err(format!("源分支 {} 不存在", spec.head_branch));
        };
        let Some(mb) = self.merge_base(full_name, &target, &head).await? else {
            return Err("两分支无共同祖先".to_owned());
        };
        if mb == head {
            return Ok(MergeOutcome::UpToDate);
        }

        let new_commit = match spec.method {
            MergeMethod::Merge => {
                // merge-tree:exit 0 首行 = 树;exit 1 = 冲突(其后为冲突文件)
                let out = Command::new("git")
                    .arg("--git-dir")
                    .arg(self.repo_path(full_name))
                    .args(["merge-tree", "--write-tree", "--name-only", &target, &head])
                    .output()
                    .await
                    .map_err(|e| e.to_string())?;
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                if out.status.code() == Some(1) {
                    let files: Vec<String> = stdout
                        .lines()
                        .skip(1)
                        .filter(|l| !l.trim().is_empty())
                        .map(str::to_owned)
                        .collect();
                    return Ok(MergeOutcome::Conflict(files));
                }
                if !out.status.success() {
                    return Err(format!(
                        "merge-tree 失败: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                let tree = stdout.lines().next().unwrap_or_default().trim().to_owned();
                self.commit_env(
                    full_name,
                    spec,
                    vec![
                        "commit-tree".to_owned(),
                        tree,
                        "-p".to_owned(),
                        target.clone(),
                        "-p".to_owned(),
                        head.clone(),
                    ],
                )
                .await?
            }
            MergeMethod::Squash => {
                let tree = self
                    .git(full_name, &["rev-parse", &format!("{head}^{{tree}}")])
                    .await?
                    .trim()
                    .to_owned();
                self.commit_env(
                    full_name,
                    spec,
                    vec![
                        "commit-tree".to_owned(),
                        tree,
                        "-p".to_owned(),
                        target.clone(),
                    ],
                )
                .await?
            }
            MergeMethod::Rebase => {
                let tmp = std::env::temp_dir()
                    .join(format!("codeplan-rebase-{}", uuid::Uuid::new_v4().simple()));
                let cleanup = || {
                    let _ = std::fs::remove_dir_all(&tmp);
                };
                self.git(
                    full_name,
                    &[
                        "worktree",
                        "add",
                        "--detach",
                        &tmp.to_string_lossy(),
                        &target,
                    ],
                )
                .await
                .map_err(|e| format!("worktree 创建失败: {e}"))?;
                let pick = Command::new("git")
                    .arg("-C")
                    .arg(&tmp)
                    .args(["cherry-pick", &format!("{mb}..{head}")])
                    .env("GIT_AUTHOR_NAME", &spec.committer_name)
                    .env("GIT_AUTHOR_EMAIL", &spec.committer_email)
                    .env("GIT_COMMITTER_NAME", &spec.committer_name)
                    .env("GIT_COMMITTER_EMAIL", &spec.committer_email)
                    .output()
                    .await
                    .map_err(|e| e.to_string());
                match pick {
                    Ok(o) if o.status.success() => {}
                    Ok(o) => {
                        let _ = Command::new("git")
                            .arg("-C")
                            .arg(&tmp)
                            .args(["cherry-pick", "--abort"])
                            .output()
                            .await;
                        let _ = self
                            .git(
                                full_name,
                                &["worktree", "remove", "--force", &tmp.to_string_lossy()],
                            )
                            .await;
                        cleanup();
                        let err = String::from_utf8_lossy(&o.stderr).trim().to_owned();
                        return Err(format!("rebase 重放冲突: {err}"));
                    }
                    Err(e) => {
                        cleanup();
                        return Err(e.to_string());
                    }
                }
                let new_head = self.git_in(&tmp, &["rev-parse", "HEAD"]).await?;
                let _ = self
                    .git(
                        full_name,
                        &["worktree", "remove", "--force", &tmp.to_string_lossy()],
                    )
                    .await;
                cleanup();
                new_head
            }
        };

        self.advance_target(full_name, &spec.target_branch, &target, &new_commit)
            .await?;
        Ok(MergeOutcome::Merged(new_commit))
    }

    async fn create_tag(
        &self,
        full_name: &str,
        name: &str,
        target: &str,
        message: &str,
    ) -> Result<(), String> {
        // 附注标签:message 经 stdin 传入避免转义问题
        let mut cmd = Command::new("git");
        cmd.arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(["tag", "-a", name, "-m", message, target])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "创建标签失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }

    async fn delete_tag(&self, full_name: &str, name: &str) -> Result<(), String> {
        let _ = self.git(full_name, &["tag", "-d", name]).await?; // 幂等:不存在时 git 报错,忽略
        Ok(())
    }

    async fn list_tags(&self, full_name: &str) -> Result<Vec<TagBrief>, String> {
        let out = self
            .git(
                full_name,
                &[
                    "for-each-ref",
                    "refs/tags",
                    "--sort=-creatordate",
                    "--format=%(refname:short)\x1f%(if)%(*objectname)%(then)%(*objectname)%(else)%(objectname)%(end)%1f%(committerdate:iso8601)",
                ],
            )
            .await?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut it = l.split('\x1f');
                TagBrief {
                    name: it.next().unwrap_or_default().to_owned(),
                    commit_sha: it.next().unwrap_or_default().to_owned(),
                    created_at: it.next().unwrap_or_default().to_owned(),
                }
            })
            .collect())
    }

    async fn raw_commit(&self, full_name: &str, sha: &str) -> Result<Option<String>, String> {
        // 不经 trim:签名 payload 必须完整保留尾部换行
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(["cat-file", "commit", sha])
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    async fn file_content(
        &self,
        full_name: &str,
        reference: &str,
        path: &str,
    ) -> Result<Option<String>, String> {
        self.git_opt(full_name, &["show", &format!("{reference}:{path}")])
            .await
    }

    async fn is_ancestor(&self, full_name: &str, a: &str, b: &str) -> Result<bool, String> {
        if a == b {
            return Ok(true);
        }
        // `merge-base --is-ancestor`:exit 0 = 是祖先,1 = 否
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(self.repo_path(full_name))
            .args(["merge-base", "--is-ancestor", a, b])
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(format!(
                "is-ancestor 判定失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    }
}

impl CliGitAccess {
    /// 任意目录下执行 git(用于 worktree 内)。
    async fn git_in(&self, dir: &std::path::Path, args: &[&str]) -> Result<String, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .await
            .map_err(|e| format!("git 启动失败: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git {:?} 失败: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
}
