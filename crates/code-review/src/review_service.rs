//! 评审流用例(T-204):行内评论锚定校验、通过/驳回规则、状态检查聚合。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::{EventBus, EventEnvelope};
use codeplan_kernel::git::{GitAccess, GitRepoLookup};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::RepoId;
use serde_json::json;

use crate::domain::{PrId, PrState, PullRequestRepository};
use crate::protection::{BranchProtection, NewProtection, ProtectionRepository};
use crate::review::{
    CheckState, CommentId, CommentRepository, InlineAnchor, NewComment, NewReview, NewStatus,
    PrComment, PrReview, ReviewRepository, ReviewSummary, StatusRepository, StatusSummary, Verdict,
    aggregate_reviews,
};
use crate::service::DOMAIN_CODE;

/// 评审流服务。
pub struct ReviewService {
    prs: Arc<dyn PullRequestRepository>,
    comments: Arc<dyn CommentRepository>,
    reviews: Arc<dyn ReviewRepository>,
    statuses: Arc<dyn StatusRepository>,
    protections: Arc<dyn ProtectionRepository>,
    reviewers: Arc<dyn crate::review::ReviewerRepository>,
    identity: Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
    lookup: Arc<dyn GitRepoLookup>,
    git: Arc<dyn GitAccess>,
    authz: Arc<dyn Authz>,
    events: Arc<dyn EventBus>,
}

impl ReviewService {
    /// 构造。
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        prs: Arc<dyn PullRequestRepository>,
        comments: Arc<dyn CommentRepository>,
        reviews: Arc<dyn ReviewRepository>,
        statuses: Arc<dyn StatusRepository>,
        protections: Arc<dyn ProtectionRepository>,
        reviewers: Arc<dyn crate::review::ReviewerRepository>,
        identity: Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
        lookup: Arc<dyn GitRepoLookup>,
        git: Arc<dyn GitAccess>,
        authz: Arc<dyn Authz>,
        events: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            prs,
            comments,
            reviews,
            statuses,
            protections,
            reviewers,
            identity,
            lookup,
            git,
            authz,
            events,
        }
    }

    async fn repo_or_404(
        &self,
        full_name: &str,
    ) -> Result<codeplan_kernel::git::RepoRef, AppError> {
        self.lookup
            .by_full_name(full_name)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "仓库不存在"))
    }

    async fn ensure(
        &self,
        user: &CurrentUser,
        action: Action,
        repo_id: RepoId,
    ) -> Result<(), AppError> {
        let reference = ResourceRef::Repo(RepoId::from_uuid(repo_id.as_uuid()));
        let ok = self
            .authz
            .check(user.id, action, &reference)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e.to_string())))?;
        if ok {
            Ok(())
        } else {
            Err(AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
        }
    }

    /// 取 PR 并校验仓库读权限;返回 (pr_id, repo_id, source_branch)。
    async fn resolve_pr(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<(PrId, RepoId, String), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        let pr = self
            .prs
            .find_by_number(RepoId::from_uuid(repo.id.as_uuid()), number)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "PR 不存在"))?;
        Ok((
            pr.id,
            RepoId::from_uuid(repo.id.as_uuid()),
            pr.source_branch,
        ))
    }

    /// PR 当前源分支头 SHA。
    async fn head_sha(
        &self,
        full_name: &str,
        source_branch: &str,
    ) -> Result<Option<String>, AppError> {
        self.git
            .resolve_ref(full_name, source_branch)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))
    }

    // ---------- 评论 ----------

    /// 发表评论(无锚定 = 对话流;有锚定 = 行内,文件必须出现在当前 diff)。
    ///
    /// # Errors
    /// 不存在 `CR-404`;锚定非法/内容为空 `CR-400`。
    pub async fn add_comment(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        body: &str,
        anchor: Option<InlineAnchor>,
    ) -> Result<PrComment, AppError> {
        let (pr_id, _repo_id, source_branch) = self.resolve_pr(actor, full_name, number).await?;
        if body.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "评论内容不能为空"));
        }
        if anchor.as_ref().is_some_and(|a| a.line <= 0) {
            return Err(AppError::invalid(DOMAIN_CODE, "行号必须大于 0"));
        }
        let anchor_sha = self.head_sha(full_name, &source_branch).await?;
        // 行内锚定:文件必须出现在当前 diff 中
        if let Some(a) = &anchor {
            let pr = self
                .prs
                .find_by_id(pr_id)
                .await?
                .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "PR 不存在"))?;
            let patch = self
                .git
                .diff_patch(full_name, &pr.target_branch, &source_branch)
                .await
                .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?;
            let files = diff_files(&patch);
            if !files.iter().any(|f| f == &a.file_path) {
                return Err(AppError::invalid(
                    DOMAIN_CODE,
                    format!("文件 {} 不在当前 diff 中", a.file_path),
                ));
            }
        }
        let comment = self
            .comments
            .insert(&NewComment {
                pr_id,
                author: actor.id,
                body: body.to_owned(),
                anchor,
                anchor_sha,
            })
            .await?;
        self.publish(
            "pr.comment_created",
            pr_id,
            json!({
                "comment_id": comment.id.as_uuid(),
                "author": actor.id.as_uuid(),
                "inline": comment.file_path.is_some(),
            }),
        )
        .await?;
        Ok(comment)
    }

    /// PR 全部评论。
    ///
    /// # Errors
    /// 不存在 `CR-404`。
    pub async fn list_comments(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<Vec<PrComment>, AppError> {
        let (pr_id, _, _) = self.resolve_pr(actor, full_name, number).await?;
        self.comments.list_by_pr(pr_id).await
    }

    /// 删除评论(作者本人或仓库 Write)。
    ///
    /// # Errors
    /// 不存在 `CR-404`;无权限 `CR-403`。
    pub async fn delete_comment(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        comment_id: CommentId,
    ) -> Result<(), AppError> {
        let (pr_id, repo_id, _) = self.resolve_pr(actor, full_name, number).await?;
        let comments = self.comments.list_by_pr(pr_id).await?;
        let comment = comments
            .into_iter()
            .find(|c| c.id == comment_id)
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "评论不存在"))?;
        if comment.author != actor.id {
            self.ensure(actor, Action::Write, repo_id).await?;
        }
        self.comments.delete(comment_id).await?;
        self.publish(
            "pr.comment_deleted",
            pr_id,
            json!({"comment_id": comment_id.as_uuid(), "actor": actor.id.as_uuid()}),
        )
        .await
    }

    // ---------- 评审 ----------

    /// 提交评审。通过/驳回要求:PR 开放中、非本人 PR;仅评论随时可发。
    ///
    /// # Errors
    /// 不存在 `CR-404`;状态非法 `CR-409`;自评 `CR-400`。
    pub async fn post_review(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        verdict: Verdict,
        body: &str,
    ) -> Result<PrReview, AppError> {
        let (pr_id, _repo_id, source_branch) = self.resolve_pr(actor, full_name, number).await?;
        let pr = self.find_pr(pr_id).await?;
        if verdict != Verdict::Comment {
            if pr.state != PrState::Open {
                return Err(AppError::conflict(DOMAIN_CODE, "PR 不在开放状态,不可评审"));
            }
            if pr.author == actor.id {
                return Err(AppError::invalid(
                    DOMAIN_CODE,
                    "不能对自己发起的 PR 通过/驳回",
                ));
            }
        }
        let head = self.head_sha(full_name, &source_branch).await?;
        let review = self
            .reviews
            .insert(&NewReview {
                pr_id,
                reviewer: actor.id,
                verdict,
                body: body.to_owned(),
                head_sha: head,
            })
            .await?;
        self.publish(
            "pr.review_posted",
            pr_id,
            json!({
                "review_id": review.id.as_uuid(),
                "reviewer": actor.id.as_uuid(),
                "verdict": verdict.as_str(),
            }),
        )
        .await?;
        Ok(review)
    }

    /// PR 全部评审记录。
    ///
    /// # Errors
    /// 不存在 `CR-404`。
    pub async fn list_reviews(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<Vec<PrReview>, AppError> {
        let (pr_id, _, _) = self.resolve_pr(actor, full_name, number).await?;
        self.reviews.list_by_pr(pr_id).await
    }

    /// 评审聚合:每人取最新结论。
    ///
    /// # Errors
    /// 不存在 `CR-404`。
    pub async fn review_summary(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<ReviewSummary, AppError> {
        let (pr_id, _, _) = self.resolve_pr(actor, full_name, number).await?;
        let reviews = self.reviews.list_by_pr(pr_id).await?;
        Ok(aggregate_reviews(&reviews))
    }

    // ---------- 状态检查 ----------

    /// 上报提交状态(Write 权限,CI 用)。
    ///
    /// # Errors
    /// 不存在 `CR-404`;参数非法 `CR-400`;无权限 `CR-403`。
    #[allow(clippy::too_many_arguments)]
    pub async fn set_status(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        commit_sha: &str,
        context: &str,
        state: CheckState,
        description: &str,
        target_url: Option<String>,
    ) -> Result<codeplan_kernel::git::RepoRef, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        let repo_id = RepoId::from_uuid(repo.id.as_uuid());
        self.ensure(actor, Action::Write, repo_id).await?;
        if commit_sha.len() < 7 || !commit_sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AppError::invalid(DOMAIN_CODE, "commit_sha 非法"));
        }
        if context.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "context 不能为空"));
        }
        self.statuses
            .upsert(&NewStatus {
                repo_id,
                commit_sha: commit_sha.to_lowercase(),
                context: context.trim().to_owned(),
                state,
                description: description.to_owned(),
                target_url,
            })
            .await?;
        self.publish(
            "commit.status_updated",
            PrId::from_uuid(uuid::Uuid::nil()),
            json!({
                "repo_id": repo_id.as_uuid(),
                "full_name": full_name,
                "commit_sha": commit_sha.to_lowercase(),
                "context": context.trim(),
                "state": state.as_str(),
            }),
        )
        .await?;
        Ok(repo)
    }

    /// PR 源分支头的状态检查聚合。
    ///
    /// # Errors
    /// 不存在 `CR-404`;分支缺失 `CR-400`。
    pub async fn pr_status(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<StatusSummary, AppError> {
        let (_, repo_id, source_branch) = self.resolve_pr(actor, full_name, number).await?;
        let head = self
            .head_sha(full_name, &source_branch)
            .await?
            .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "源分支不存在"))?;
        let checks = self.statuses.list_by_commit(repo_id, &head).await?;
        Ok(StatusSummary::aggregate(checks))
    }

    /// 系统级状态上报(无 RBAC;CI 引擎事件桥用,T-303)。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;参数非法 `CR-400`。
    pub async fn set_status_by_system(
        &self,
        full_name: &str,
        commit_sha: &str,
        context: &str,
        state: CheckState,
        description: &str,
        target_url: Option<String>,
    ) -> Result<codeplan_kernel::git::RepoRef, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        let repo_id = RepoId::from_uuid(repo.id.as_uuid());
        self.statuses
            .upsert(&NewStatus {
                repo_id,
                commit_sha: commit_sha.to_lowercase(),
                context: context.trim().to_owned(),
                state,
                description: description.to_owned(),
                target_url,
            })
            .await?;
        self.events
            .publish(vec![EventEnvelope::new(
                "commit.status_updated",
                serde_json::json!({
                    "repo_id": repo_id.as_uuid(),
                    "full_name": full_name,
                    "commit_sha": commit_sha.to_lowercase(),
                    "context": context.trim(),
                    "state": state.as_str(),
                    "actor": "system",
                }),
            )])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?;
        Ok(repo)
    }

    /// 任意提交的状态检查聚合(Read 权限)。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;无权限 `CR-403`。
    pub async fn commit_statuses(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        commit_sha: &str,
    ) -> Result<StatusSummary, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        let checks = self
            .statuses
            .list_by_commit(RepoId::from_uuid(repo.id.as_uuid()), commit_sha)
            .await?;
        Ok(StatusSummary::aggregate(checks))
    }

    /// 写入/更新分支保护规则(仓库 Admin 权限)。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;参数非法 `CR-400`;无权限 `CR-403`。
    #[allow(clippy::too_many_arguments)]
    pub async fn set_protection(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        branch_pattern: &str,
        min_approvals: i32,
        required_contexts: Vec<String>,
        block_force_push: bool,
        block_direct_push: bool,
        require_code_owner: bool,
    ) -> Result<BranchProtection, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Admin, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        if branch_pattern.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "分支模式不能为空"));
        }
        if !(0..=10).contains(&min_approvals) {
            return Err(AppError::invalid(DOMAIN_CODE, "min_approvals 取值 0-10"));
        }
        let rule = self
            .protections
            .upsert(&NewProtection {
                repo_id: RepoId::from_uuid(repo.id.as_uuid()),
                branch_pattern: branch_pattern.trim().to_owned(),
                min_approvals,
                required_contexts,
                block_force_push,
                block_direct_push,
                require_code_owner,
            })
            .await?;
        self.events
            .publish(vec![EventEnvelope::new(
                "repo.branch_protected",
                json!({
                    "repo_id": repo.id.as_uuid(),
                    "full_name": full_name,
                    "branch_pattern": rule.branch_pattern,
                    "actor": actor.id.as_uuid(),
                }),
            )])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))?;
        Ok(rule)
    }

    /// 保护规则列表(Read 权限)。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;无权限 `CR-403`。
    pub async fn list_protections(
        &self,
        actor: &CurrentUser,
        full_name: &str,
    ) -> Result<Vec<BranchProtection>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.protections
            .list_by_repo(RepoId::from_uuid(repo.id.as_uuid()))
            .await
    }

    /// 删除保护规则(仓库 Admin 权限)。
    ///
    /// # Errors
    /// 仓库/规则不存在 `CR-404`;无权限 `CR-403`。
    pub async fn delete_protection(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        branch_pattern: &str,
    ) -> Result<(), AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Admin, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.protections
            .delete(RepoId::from_uuid(repo.id.as_uuid()), branch_pattern)
            .await?;
        self.events
            .publish(vec![EventEnvelope::new(
                "repo.branch_unprotected",
                json!({
                    "repo_id": repo.id.as_uuid(),
                    "full_name": full_name,
                    "branch_pattern": branch_pattern,
                    "actor": actor.id.as_uuid(),
                }),
            )])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))
    }

    /// CODEOWNERS 内容与解析结果(目标引用,默认分支可用 ?ref= 覆盖)。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;无权限 `CR-403`。
    pub async fn codeowners_info(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        reference: Option<&str>,
    ) -> Result<
        (
            Option<String>,
            Option<String>,
            Vec<crate::owners::CodeOwnerRule>,
        ),
        AppError,
    > {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        let reference = reference
            .map(str::to_owned)
            .unwrap_or_else(|| repo.default_branch.clone());
        let mut found: Option<(String, String)> = None;
        for path in crate::owners::CODEOWNERS_PATHS {
            if let Some(content) = self
                .git
                .file_content(full_name, &reference, path)
                .await
                .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            {
                found = Some((path.to_owned(), content));
            }
        }
        Ok(match found {
            Some((path, raw)) => {
                let rules = crate::owners::parse_codeowners(&raw);
                (Some(path), Some(raw), rules)
            }
            None => (None, None, Vec::new()),
        })
    }

    /// PR 评审人列表(附当前评审结论)。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`。
    pub async fn list_reviewers(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<Vec<(crate::review::PrReviewer, Option<crate::review::Verdict>)>, AppError> {
        let (pr_id, _, _) = self.resolve_pr(actor, full_name, number).await?;
        let reviewers = self.reviewers.list_by_pr(pr_id).await?;
        let reviews = self.reviews.list_by_pr(pr_id).await?;
        let mut latest: std::collections::HashMap<codeplan_kernel::ids::UserId, Verdict> =
            std::collections::HashMap::new();
        for r in &reviews {
            latest.entry(r.reviewer).or_insert(r.verdict);
        }
        Ok(reviewers
            .into_iter()
            .map(|r| {
                let v = latest.get(&r.reviewer).copied();
                (r, v)
            })
            .collect())
    }

    /// 添加评审人(作者本人或 Write 权限)。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`;用户不存在 `CR-400`;无权限 `CR-403`。
    pub async fn add_reviewer(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        username: &str,
    ) -> Result<
        (
            crate::review::PrReviewer,
            codeplan_kernel::lookup::UserBrief,
        ),
        AppError,
    > {
        let (pr_id, repo_id, _) = self.resolve_pr(actor, full_name, number).await?;
        let pr = self.find_pr(pr_id).await?;
        if pr.author != actor.id {
            self.ensure(actor, Action::Write, repo_id).await?;
        }
        let user = self
            .identity
            .by_username(username)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .ok_or_else(|| AppError::invalid(DOMAIN_CODE, format!("用户 {username} 不存在")))?;
        let reviewer = self
            .reviewers
            .upsert(&crate::review::NewReviewer {
                pr_id,
                reviewer: user.id,
                added_by: Some(actor.id),
            })
            .await?;
        self.publish(
            "pr.reviewer_added",
            pr_id,
            serde_json::json!({"reviewer": user.id.as_uuid(), "actor": actor.id.as_uuid()}),
        )
        .await?;
        Ok((reviewer, user))
    }

    /// 移除评审人(作者本人或 Write 权限)。
    ///
    /// # Errors
    /// 不存在 `CR-404`;无权限 `CR-403`。
    pub async fn remove_reviewer(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        username: &str,
    ) -> Result<(), AppError> {
        let (pr_id, repo_id, _) = self.resolve_pr(actor, full_name, number).await?;
        let pr = self.find_pr(pr_id).await?;
        if pr.author != actor.id {
            self.ensure(actor, Action::Write, repo_id).await?;
        }
        let user = self
            .identity
            .by_username(username)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .ok_or_else(|| AppError::invalid(DOMAIN_CODE, format!("用户 {username} 不存在")))?;
        self.reviewers.delete(pr_id, user.id).await?;
        self.publish(
            "pr.reviewer_removed",
            pr_id,
            serde_json::json!({"reviewer": user.id.as_uuid(), "actor": actor.id.as_uuid()}),
        )
        .await
    }

    async fn find_pr(&self, pr_id: PrId) -> Result<crate::domain::PullRequest, AppError> {
        self.prs
            .find_by_id(pr_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "PR 不存在"))
    }

    async fn publish(
        &self,
        event_type: &str,
        pr_id: PrId,
        mut extra: serde_json::Value,
    ) -> Result<(), AppError> {
        if let serde_json::Value::Object(map) = &mut extra {
            map.insert("pr_id".into(), json!(pr_id.as_uuid()));
        }
        self.events
            .publish(vec![EventEnvelope::new(event_type, extra)])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))
    }
}

/// 从 unified diff 提取文件路径列表。
#[must_use]
pub fn diff_files(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| l.strip_prefix("diff --git a/"))
        .filter_map(|rest| rest.split_once(" b/").map(|(_, p)| p.to_owned()))
        .collect()
}
