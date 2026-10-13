//! Git 域用例:仓库生命周期(创建/归档/设置)+ 权限 + 事件。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::EventEnvelope;
use codeplan_kernel::identity::CurrentUser;
use serde_json::json;

use crate::DOMAIN_CODE;
use crate::domain::{BareRepoStore, NewRepository, Repository, RepositoryRepository};

/// Git 域服务。
pub struct GitService {
    repos: Arc<dyn RepositoryRepository>,
    bare: Arc<dyn BareRepoStore>,
    authz: Arc<dyn Authz>,
    events: Arc<dyn codeplan_kernel::events::EventBus>,
}

/// 仓库键规则:小写字母/数字/连字符,2~64 位。
#[must_use]
pub fn valid_repo_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

impl GitService {
    /// 构造。
    #[must_use]
    pub fn new(
        repos: Arc<dyn RepositoryRepository>,
        bare: Arc<dyn BareRepoStore>,
        authz: Arc<dyn Authz>,
        events: Arc<dyn codeplan_kernel::events::EventBus>,
    ) -> Self {
        Self {
            repos,
            bare,
            authz,
            events,
        }
    }

    /// 创建仓库:元数据 + 裸仓库 + `repo.created` 事件(identity 监听授予创建者 admin)。
    ///
    /// # Errors
    /// key 非法 `GIT-400`;冲突 `GIT-409`;无项目权限 `GIT-403`;git 初始化失败 `GIT-500`。
    #[allow(clippy::too_many_arguments)]
    pub async fn create_repository(
        &self,
        actor: &CurrentUser,
        owner_key: &str,
        key: &str,
        name: &str,
        description: &str,
        visibility: crate::domain::Visibility,
        default_branch: &str,
        project_id: Option<codeplan_kernel::ids::ProjectId>,
    ) -> Result<Repository, AppError> {
        if !valid_repo_key(key) {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "仓库键须为小写字母/数字/连字符,2~64 位",
            ));
        }
        if let Some(pid) = project_id {
            let reference = ResourceRef::Project(pid);
            self.authz
                .check(actor.id, Action::Write, &reference)
                .await
                .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
                .then_some(())
                .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无项目写权限,无法关联仓库"))?;
        }
        let full_name = format!("{owner_key}/{key}");
        let new = NewRepository {
            key: key.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            visibility,
            default_branch: default_branch.to_owned(),
            owner: actor.id,
            project_id,
        };
        let repo = self.repos.create(&new, &full_name).await?;
        if let Err(e) = self.bare.init_bare(&full_name, &repo.default_branch).await {
            // 裸仓库初始化失败:归档元数据保持一致(不做物理删除,人工可查)
            let _ = self.repos.set_archived(repo.id, true).await;
            return Err(AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)));
        }
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "repo.created",
                    json!({
                        "repo_id": repo.id.as_uuid(),
                        "full_name": repo.full_name,
                        "creator_id": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(repo)
    }

    /// 仓库列表(未归档)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_repositories(&self) -> Result<Vec<Repository>, AppError> {
        Ok(self
            .repos
            .list()
            .await?
            .into_iter()
            .filter(|r| !r.archived)
            .collect())
    }

    /// 按全名取仓库(归档的返回 Err)。
    ///
    /// # Errors
    /// 不存在或已归档 `GIT-404`。
    pub async fn repository_by_full_name(&self, full_name: &str) -> Result<Repository, AppError> {
        let repo = self
            .repos
            .find_by_full_name(full_name)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "仓库不存在"))?;
        if repo.archived {
            return Err(AppError::not_found(DOMAIN_CODE, "仓库已归档"));
        }
        Ok(repo)
    }

    /// 设置默认分支(需要仓库管理权限)。
    ///
    /// # Errors
    /// 无权限 `GIT-403`;分支名非法 `GIT-400`。
    pub async fn set_default_branch(
        &self,
        actor: &CurrentUser,
        repo: &Repository,
        branch: &str,
    ) -> Result<(), AppError> {
        if branch.is_empty() || branch.contains(' ') || branch.starts_with('-') {
            return Err(AppError::invalid(DOMAIN_CODE, "非法分支名"));
        }
        self.ensure(actor, Action::Admin, repo).await?;
        self.repos.set_default_branch(repo.id, branch).await
    }

    /// 归档/恢复(需要仓库管理权限;归档保留裸仓库)。
    ///
    /// # Errors
    /// 无权限 `GIT-403`。
    pub async fn set_archived(
        &self,
        actor: &CurrentUser,
        repo: &Repository,
        archived: bool,
    ) -> Result<(), AppError> {
        self.ensure(actor, Action::Admin, repo).await?;
        self.repos.set_archived(repo.id, archived).await
    }

    /// 权限检查透传(HTTP 传输用)。
    ///
    /// # Errors
    /// 无权限 `GIT-403`;后端故障。
    pub async fn ensure_action(
        &self,
        actor: &CurrentUser,
        action: Action,
        reference: &ResourceRef,
    ) -> Result<(), AppError> {
        self.authz
            .check(actor.id, action, reference)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .then_some(())
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }

    /// 发布 `commit.pushed`(post-receive 钩子回调;refs 为钩子原始行
    /// `old new ref`),payload 附各引用新提交的消息(T-207 提交关联)。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;事件发布失败。
    pub async fn publish_push(&self, full_name: &str, refs: &[String]) -> Result<(), AppError> {
        let repo = self
            .repos
            .find_by_full_name(full_name)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "仓库不存在"))?;
        let mut commits: Vec<serde_json::Value> = Vec::new();
        for line in refs {
            let Some(parsed) = parse_push_ref(line) else {
                continue;
            };
            for (sha, message) in self.pushed_commits(full_name, &parsed).await {
                commits.push(json!({"sha": sha, "message": message, "ref": parsed.ref_name}));
            }
        }
        self.events
            .publish(vec![EventEnvelope::new(
                "commit.pushed",
                json!({
                    "repo_id": repo.id.as_uuid(),
                    "full_name": repo.full_name,
                    "default_branch": repo.default_branch,
                    "refs": refs,
                    "commits": commits,
                }),
            )])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))
    }

    /// 提取一次引用更新引入的提交(sha + 完整消息)。
    async fn pushed_commits(
        &self,
        full_name: &str,
        parsed: &PushedRef<'_>,
    ) -> Vec<(String, String)> {
        let path = self.bare.repo_path(full_name);
        // 新建引用:取分支顶端最近提交;普通更新:old..new;删除:无
        let range = if is_zero_sha(parsed.old) {
            format!("-50|{}", parsed.new)
        } else {
            format!("200|{}..{}", parsed.old, parsed.new)
        };
        let Some((limit, spec)) = range.split_once('|') else {
            return Vec::new();
        };
        let out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&path)
            .args([
                "--no-pager",
                "log",
                "--format=%H\x1f%s\x1f%b%x1e",
                "--max-count",
                limit,
                spec,
            ])
            .output()
            .await;
        let Ok(out) = out else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&out.stdout)
            .split('\x1e')
            .filter_map(|record| {
                let record = record.trim_matches('\n');
                if record.is_empty() {
                    return None;
                }
                let mut it = record.split('\x1f');
                Some((
                    it.next()?.to_owned(),
                    format!(
                        "{}\n{}",
                        it.next().unwrap_or_default(),
                        it.next().unwrap_or_default()
                    )
                    .trim()
                    .to_owned(),
                ))
            })
            .collect()
    }

    async fn ensure(
        &self,
        actor: &CurrentUser,
        action: Action,
        repo: &Repository,
    ) -> Result<(), AppError> {
        let reference =
            ResourceRef::Repo(codeplan_kernel::ids::RepoId::from_uuid(repo.id.as_uuid()));
        self.authz
            .check(actor.id, action, &reference)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .then_some(())
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }
}

/// 一次引用更新的三元组。
pub struct PushedRef<'a> {
    /// 旧 SHA。
    pub old: &'a str,
    /// 新 SHA。
    pub new: &'a str,
    /// 引用全名。
    pub ref_name: &'a str,
}

/// 解析钩子原始行 `old new ref`。
#[must_use]
pub fn parse_push_ref(line: &str) -> Option<PushedRef<'_>> {
    let mut it = line.split_whitespace();
    let old = it.next()?;
    let new = it.next()?;
    let ref_name = it.next()?;
    Some(PushedRef { old, new, ref_name })
}

/// 全零 SHA(新建/删除标记)。
#[must_use]
pub fn is_zero_sha(sha: &str) -> bool {
    sha.bytes().all(|b| b == b'0') || sha.is_empty()
}

#[cfg(test)]
mod push_parse_tests {
    use super::*;

    #[test]
    fn parse_push_ref_variants() {
        let r = parse_push_ref("0000abc 1234abc refs/heads/main").unwrap();
        assert_eq!(r.ref_name, "refs/heads/main");
        assert!(
            is_zero_sha(r.old) || !r.old.starts_with("00000"),
            "兼容任意零串"
        );
        assert!(parse_push_ref("only-two tokens").is_none());
        assert!(is_zero_sha("0000000000000000000000000000000000000000"));
        assert!(!is_zero_sha("1234abc"));
    }
}
