//! Release 用例(T-208):标签管理、发布(自动 Notes)、附件。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::{EventBus, EventEnvelope};
use codeplan_kernel::git::{CommitBrief, GitAccess};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::RepoId;
use serde_json::json;

use codeplan_storage::ObjectStore;

use crate::domain::RepositoryRepository;
use crate::release::{NewRelease, Release, ReleaseAsset, ReleaseId, ReleaseRepository};

/// 附件大小上限(512MB)。
pub const MAX_ASSET_SIZE: usize = 512 * 1024 * 1024;

/// Release 服务。
pub struct ReleaseService {
    releases: Arc<dyn ReleaseRepository>,
    repos: Arc<dyn RepositoryRepository>,
    git: Arc<dyn GitAccess>,
    authz: Arc<dyn Authz>,
    store: Arc<ObjectStore>,
    events: Arc<dyn EventBus>,
}

impl ReleaseService {
    /// 构造。
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        releases: Arc<dyn ReleaseRepository>,
        repos: Arc<dyn RepositoryRepository>,
        git: Arc<dyn GitAccess>,
        authz: Arc<dyn Authz>,
        store: Arc<ObjectStore>,
        events: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            releases,
            repos,
            git,
            authz,
            store,
            events,
        }
    }

    async fn repo_or_404(&self, full_name: &str) -> Result<crate::domain::Repository, AppError> {
        self.repos
            .find_by_full_name(full_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "仓库不存在"))
    }

    async fn ensure(
        &self,
        user: &CurrentUser,
        action: Action,
        repo: &crate::domain::Repository,
    ) -> Result<(), AppError> {
        let reference = ResourceRef::Repo(RepoId::from_uuid(repo.id.as_uuid()));
        let ok = self
            .authz
            .check(user.id, action, &reference)
            .await
            .map_err(|e| {
                AppError::internal(crate::DOMAIN_CODE).with_details(json!(e.to_string()))
            })?;
        if ok {
            Ok(())
        } else {
            Err(AppError::forbidden(crate::DOMAIN_CODE, "无权限执行该操作"))
        }
    }

    // ---------- 标签 ----------

    /// 创建附注标签。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;目标不存在/标签已存在 `GIT-400`;无权限 `GIT-403`。
    pub async fn create_tag(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        name: &str,
        target: Option<&str>,
        message: &str,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        if repo.archived {
            return Err(AppError::forbidden(crate::DOMAIN_CODE, "仓库已归档"));
        }
        self.ensure(actor, Action::Write, &repo).await?;
        if name.trim().is_empty() || name.contains(char::is_whitespace) {
            return Err(AppError::invalid(crate::DOMAIN_CODE, "标签名非法"));
        }
        let target = target.unwrap_or(&repo.default_branch);
        self.git
            .create_tag(full_name, name, target, message)
            .await
            .map_err(|e| AppError::invalid(crate::DOMAIN_CODE, e))
    }

    /// 删除标签。
    ///
    /// # Errors
    /// 无权限 `GIT-403`。
    pub async fn delete_tag(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        name: &str,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, &repo).await?;
        self.git.delete_tag(full_name, name).await.map_err(apperr)
    }

    /// 标签列表。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn list_tags(
        &self,
        actor: &CurrentUser,
        full_name: &str,
    ) -> Result<Vec<codeplan_kernel::git::TagBrief>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, &repo).await?;
        self.git.list_tags(full_name).await.map_err(apperr)
    }

    // ---------- Release ----------

    /// 上一标签自动探测:指向提交是当前标签祖先、且提交时间最新的标签。
    /// (creatordate 排序对同批创建的标签不稳定,拓扑判断才可靠。)
    async fn previous_tag(&self, full_name: &str, tag_name: &str) -> Option<String> {
        let tags = self.git.list_tags(full_name).await.ok()?;
        let tag_commit = self.git.resolve_ref(full_name, tag_name).await.ok()??;
        let mut best: Option<(String, String)> = None; // (name, committerdate)
        for t in tags {
            if t.name == tag_name || t.commit_sha.is_empty() {
                continue;
            }
            let ancestor = self
                .git
                .is_ancestor(full_name, &t.commit_sha, &tag_commit)
                .await
                .ok()?;
            if !ancestor {
                continue;
            }
            match &best {
                Some((_, d)) if *d >= t.created_at => {}
                _ => best = Some((t.name.clone(), t.created_at.clone())),
            }
        }
        best.map(|(n, _)| n)
    }

    /// 发布 Release:body 为空时按上一标签以来的提交自动生成 Notes。
    ///
    /// # Errors
    /// 仓库/标签不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn publish(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
        name: &str,
        body: Option<&str>,
        previous_tag: Option<&str>,
    ) -> Result<Release, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        if repo.archived {
            return Err(AppError::forbidden(crate::DOMAIN_CODE, "仓库已归档"));
        }
        self.ensure(actor, Action::Write, &repo).await?;
        // 标签必须存在
        self.git
            .resolve_ref(full_name, tag_name)
            .await
            .map_err(apperr)?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "标签不存在"))?;

        let (body_final, auto) = match body {
            Some(b) if !b.trim().is_empty() => (b.to_owned(), false),
            _ => {
                let prev = match previous_tag {
                    Some(p) if !p.trim().is_empty() => Some(p.to_owned()),
                    _ => self.previous_tag(full_name, tag_name).await,
                };
                let commits = self
                    .git
                    .commits_between(full_name, prev.as_deref().unwrap_or(""), tag_name)
                    .await
                    .unwrap_or_default();
                (generate_notes(tag_name, prev.as_deref(), &commits), true)
            }
        };

        let release = self
            .releases
            .create(&NewRelease {
                repo_id: RepoId::from_uuid(repo.id.as_uuid()),
                tag_name: tag_name.to_owned(),
                name: if name.trim().is_empty() {
                    tag_name.to_owned()
                } else {
                    name.to_owned()
                },
                body: body_final,
                notes_auto: auto,
                author: actor.id,
            })
            .await?;
        let _ = self
            .events
            .publish(vec![EventEnvelope::new(
                "release.published",
                json!({
                    "repo_id": repo.id.as_uuid(),
                    "full_name": full_name,
                    "tag": tag_name,
                    "name": release.name,
                    "actor": actor.id.as_uuid(),
                }),
            )])
            .await;
        Ok(release)
    }

    /// 更新标题/Notes。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn update(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
        name: Option<&str>,
        body: Option<&str>,
    ) -> Result<Release, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        self.releases.update(release.id, name, body, None).await
    }

    /// 删除 Release(连带清理附件对象)。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn delete(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        for asset in self.releases.list_assets(release.id).await? {
            let _ = self
                .store
                .delete(&asset_key(release.id, &asset.filename))
                .await;
        }
        self.releases.delete(release.id).await
    }

    /// Release 列表(带附件)。
    ///
    /// # Errors
    /// 仓库不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn list(
        &self,
        actor: &CurrentUser,
        full_name: &str,
    ) -> Result<Vec<(Release, Vec<ReleaseAsset>)>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, &repo).await?;
        let releases = self
            .releases
            .list_by_repo(RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        let mut out = Vec::with_capacity(releases.len());
        for r in releases {
            let assets = self.releases.list_assets(r.id).await?;
            out.push((r, assets));
        }
        Ok(out)
    }

    /// Release 详情。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn get(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
    ) -> Result<(Release, Vec<ReleaseAsset>), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        let assets = self.releases.list_assets(release.id).await?;
        Ok((release, assets))
    }

    // ---------- 附件 ----------

    /// 上传附件(同名覆盖)。
    ///
    /// # Errors
    /// Release 不存在 `GIT-404`;文件名非法/超大 `GIT-400`;无权限 `GIT-403`。
    pub async fn upload_asset(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<ReleaseAsset, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        validate_filename(filename)?;
        if data.len() > MAX_ASSET_SIZE {
            return Err(AppError::invalid(crate::DOMAIN_CODE, "附件超过大小上限"));
        }
        self.store
            .put(&asset_key(release.id, filename), data.clone())
            .await
            .map_err(|e| {
                AppError::internal(crate::DOMAIN_CODE).with_details(json!(e.to_string()))
            })?;
        self.releases
            .upsert_asset(release.id, filename, data.len() as i64)
            .await
    }

    /// 下载附件。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn download_asset(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
        filename: &str,
    ) -> Result<Vec<u8>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        let _ = self
            .releases
            .list_assets(release.id)
            .await?
            .iter()
            .find(|a| a.filename == filename)
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "附件不存在"))?;
        self.store
            .get(&asset_key(release.id, filename))
            .await
            .map_err(|e| AppError::internal(crate::DOMAIN_CODE).with_details(json!(e.to_string())))?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "附件内容缺失"))
    }

    /// 删除附件。
    ///
    /// # Errors
    /// 不存在 `GIT-404`;无权限 `GIT-403`。
    pub async fn delete_asset(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        tag_name: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Write, &repo).await?;
        let release = self
            .releases
            .find_by_tag(RepoId::from_uuid(repo.id.as_uuid()), tag_name)
            .await?
            .ok_or_else(|| AppError::not_found(crate::DOMAIN_CODE, "Release 不存在"))?;
        self.releases.delete_asset(release.id, filename).await?;
        let _ = self.store.delete(&asset_key(release.id, filename)).await;
        Ok(())
    }
}

/// 附件对象存储 key。
fn asset_key(release_id: ReleaseId, filename: &str) -> String {
    format!("release/{}/{}", release_id.as_uuid(), filename)
}

/// 文件名白名单校验(拒绝路径分隔与隐藏遍历)。
fn validate_filename(filename: &str) -> Result<(), AppError> {
    let ok = !filename.is_empty()
        && filename.len() <= 255
        && !filename.contains('/')
        && !filename.contains('\\')
        && filename != "."
        && filename != ".."
        && !filename.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(AppError::invalid(crate::DOMAIN_CODE, "文件名非法"))
    }
}

/// 自动生成 Release Notes(纯函数)。
#[must_use]
pub fn generate_notes(tag: &str, previous: Option<&str>, commits: &[CommitBrief]) -> String {
    let mut notes = String::from("## What's Changed\n\n");
    if commits.is_empty() {
        notes.push_str("此版本无提交变更。\n");
    } else {
        for c in commits {
            let short = &c.id[..c.id.len().min(7)];
            notes.push_str(&format!("- {} (`{}`)\n", c.message_title, short));
        }
    }
    notes.push_str("\n**Full changelog**: ");
    notes.push_str(&match previous {
        Some(prev) => format!("{prev}...{tag}"),
        None => format!("首次发布 `{tag}`"),
    });
    notes.push('\n');
    notes
}

fn apperr(e: String) -> AppError {
    AppError::internal(crate::DOMAIN_CODE).with_details(json!(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn commit(sha: &str, title: &str) -> CommitBrief {
        CommitBrief {
            id: sha.into(),
            author_name: "d".into(),
            author_email: "d@corp.test".into(),
            message_title: title.into(),
            committed_at: Utc::now(),
        }
    }

    #[test]
    fn notes_generated_from_commits() {
        let notes = generate_notes(
            "v1.0.1",
            Some("v1.0.0"),
            &[
                commit("aaaaaaaaaaaa", "fix #CL-1 登录崩溃"),
                commit("bbbbbbbbbbbb", "feat: 导出"),
            ],
        );
        assert!(notes.contains("## What's Changed"));
        assert!(notes.contains("- fix #CL-1 登录崩溃 (`aaaaaaa`)"));
        assert!(notes.contains("**Full changelog**: v1.0.0...v1.0.1"));
    }

    #[test]
    fn notes_first_release_and_empty() {
        let notes = generate_notes("v0.1.0", None, &[]);
        assert!(notes.contains("首次发布"));
        assert!(notes.contains("此版本无提交变更"));
    }

    #[test]
    fn filename_validation() {
        assert!(validate_filename("app-1.0.tar.gz").is_ok());
        assert!(validate_filename("../evil").is_err());
        assert!(validate_filename("a/b").is_err());
        assert!(validate_filename(".hidden").is_err());
        assert!(validate_filename("").is_err());
    }
}
