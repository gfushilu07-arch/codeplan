//! PR 用例编排:创建校验(分支存在/有差异)→ diff/commits 透传 →
//! 合并执行(draft 门禁 + 终态守卫)→ 事件发布。

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::EventBus;
use codeplan_kernel::git::{GitAccess, GitRepoLookup, MergeMethod, MergeSpec};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::RepoId;
use serde_json::json;

use crate::domain::{NewPullRequest, PrEdit, PrState, PullRequest, PullRequestRepository};
use crate::owners::{
    CODEOWNERS_PATHS, all_owners, diff_files_pub, owners_by_files, parse_codeowners,
};
use crate::protection::ProtectionRepository;
use crate::review::{CheckState, ReviewRepository, ReviewerRepository, StatusRepository};

/// 域错误码前缀(`CR-xxx`)。
pub const DOMAIN_CODE: &str = "CR";

/// PR 服务。
pub struct PullRequestService {
    prs: std::sync::Arc<dyn PullRequestRepository>,
    lookup: std::sync::Arc<dyn GitRepoLookup>,
    git: std::sync::Arc<dyn GitAccess>,
    authz: std::sync::Arc<dyn Authz>,
    events: std::sync::Arc<dyn EventBus>,
    protections: std::sync::Arc<dyn ProtectionRepository>,
    reviews: std::sync::Arc<dyn ReviewRepository>,
    statuses: std::sync::Arc<dyn StatusRepository>,
    reviewers: std::sync::Arc<dyn ReviewerRepository>,
    identity: std::sync::Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
}

impl PullRequestService {
    /// 构造。
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        prs: std::sync::Arc<dyn PullRequestRepository>,
        lookup: std::sync::Arc<dyn GitRepoLookup>,
        git: std::sync::Arc<dyn GitAccess>,
        authz: std::sync::Arc<dyn Authz>,
        events: std::sync::Arc<dyn EventBus>,
        protections: std::sync::Arc<dyn ProtectionRepository>,
        reviews: std::sync::Arc<dyn ReviewRepository>,
        statuses: std::sync::Arc<dyn StatusRepository>,
        reviewers: std::sync::Arc<dyn ReviewerRepository>,
        identity: std::sync::Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
    ) -> Self {
        Self {
            prs,
            lookup,
            git,
            authz,
            events,
            protections,
            reviews,
            statuses,
            reviewers,
            identity,
        }
    }

    async fn ensure(
        &self,
        user: &CurrentUser,
        action: Action,
        repo_id: RepoId,
    ) -> Result<(), AppError> {
        let reference =
            ResourceRef::Repo(codeplan_kernel::ids::RepoId::from_uuid(repo_id.as_uuid()));
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

    /// 创建 PR:源≠目标、两分支存在、有差异。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;参数非法 `CR-400`;无差异 `CR-400`;无权限 `CR-403`。
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        title: &str,
        body: &str,
        source_branch: &str,
        target_branch: Option<&str>,
        is_draft: bool,
    ) -> Result<PullRequest, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        if repo.archived {
            return Err(AppError::forbidden(DOMAIN_CODE, "仓库已归档"));
        }
        let target_branch = target_branch.unwrap_or(&repo.default_branch);
        if title.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "标题不能为空"));
        }
        if source_branch == target_branch {
            return Err(AppError::invalid(DOMAIN_CODE, "源分支与目标分支相同"));
        }
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        // 分支存在性 + 差异校验(复用 diff_patch 的 merge-base 解析)
        self.git
            .resolve_ref(full_name, source_branch)
            .await
            .map_err(apperr)?
            .ok_or_else(|| {
                AppError::invalid(DOMAIN_CODE, format!("源分支 {source_branch} 不存在"))
            })?;
        self.git
            .resolve_ref(full_name, target_branch)
            .await
            .map_err(apperr)?
            .ok_or_else(|| {
                AppError::invalid(DOMAIN_CODE, format!("目标分支 {target_branch} 不存在"))
            })?;
        let patch = self
            .git
            .diff_patch(full_name, target_branch, source_branch)
            .await
            .map_err(apperr)?;
        if patch.trim().is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "源分支相对目标没有差异"));
        }

        let pr = self
            .prs
            .insert(&NewPullRequest {
                repo_id: RepoId::from_uuid(repo.id.as_uuid()),
                title: title.trim().to_owned(),
                body: body.to_owned(),
                source_branch: source_branch.to_owned(),
                target_branch: target_branch.to_owned(),
                author: actor.id,
                is_draft,
            })
            .await?;
        self.publish(
            "pr.created",
            &pr,
            json!({
                "repo_id": repo.id.as_uuid(),
                "full_name": full_name,
                "actor": actor.id.as_uuid(),
            }),
        )
        .await?;
        // CODEOWNERS 自动指派(T-206,best-effort:失败不阻塞建 PR)
        let _ = self.auto_assign_reviewers(full_name, &pr).await;
        Ok(pr)
    }

    /// 读取 CODEOWNERS(按 GitHub 优先级取最后存在的文件)。
    async fn load_codeowners(
        &self,
        full_name: &str,
        reference: &str,
    ) -> Result<(Option<String>, Vec<crate::owners::CodeOwnerRule>), AppError> {
        let mut found: Option<(String, String)> = None;
        for path in CODEOWNERS_PATHS {
            if let Some(content) = self
                .git
                .file_content(full_name, reference, path)
                .await
                .map_err(apperr)?
            {
                found = Some((path.to_owned(), content));
            }
        }
        Ok(match found {
            Some((path, raw)) => {
                let rules = parse_codeowners(&raw);
                (Some(path), rules)
            }
            None => (None, Vec::new()),
        })
    }

    /// CODEOWNERS 自动指派:变更文件的所有者请求评审(排除作者,上限 10 人)。
    async fn auto_assign_reviewers(
        &self,
        full_name: &str,
        pr: &PullRequest,
    ) -> Result<Vec<String>, AppError> {
        let (_, rules) = self.load_codeowners(full_name, &pr.target_branch).await?;
        if rules.is_empty() {
            return Ok(Vec::new());
        }
        let patch = self
            .git
            .diff_patch(full_name, &pr.target_branch, &pr.source_branch)
            .await
            .map_err(apperr)?;
        let files = diff_files_pub(&patch);
        let covered = owners_by_files(&rules, &files);
        let mut assigned = Vec::new();
        for username in all_owners(&covered) {
            if assigned.len() >= 10 {
                break;
            }
            let Some(user) = self.identity.by_username(&username).await.map_err(apperr)? else {
                continue; // CODEOWNERS 里的用户不存在于平台,跳过
            };
            if user.id == pr.author {
                continue;
            }
            self.reviewers
                .upsert(&crate::review::NewReviewer {
                    pr_id: pr.id,
                    reviewer: user.id,
                    added_by: None,
                })
                .await?;
            assigned.push(username);
        }
        Ok(assigned)
    }

    /// PR 列表。
    ///
    /// # Errors
    /// 仓库不存在 `CR-404`;无权限 `CR-403`。
    pub async fn list(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.prs
            .list_by_repo(RepoId::from_uuid(repo.id.as_uuid()), state)
            .await
    }

    /// PR 详情。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`;无权限 `CR-403`。
    pub async fn get(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<PullRequest, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        self.ensure(actor, Action::Read, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        self.prs
            .find_by_number(RepoId::from_uuid(repo.id.as_uuid()), number)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "PR 不存在"))
    }

    /// PR 三点差异补丁。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`;分支缺失 `CR-400`。
    pub async fn diff(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<String, AppError> {
        let pr = self.get(actor, full_name, number).await?;
        self.git
            .diff_patch(full_name, &pr.target_branch, &pr.source_branch)
            .await
            .map_err(apperr)
    }

    /// PR 区间提交列表。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`;分支缺失 `CR-400`。
    pub async fn commits(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
    ) -> Result<Vec<codeplan_kernel::git::CommitBrief>, AppError> {
        let pr = self.get(actor, full_name, number).await?;
        self.git
            .commits_between(full_name, &pr.target_branch, &pr.source_branch)
            .await
            .map_err(apperr)
    }

    /// 编辑(仅作者或 Write 权限)。
    ///
    /// # Errors
    /// 仓库/PR 不存在 `CR-404`;无权限 `CR-403`;已合并 `CR-409`。
    pub async fn edit(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        edit: &PrEdit,
    ) -> Result<PullRequest, AppError> {
        let pr = self.get(actor, full_name, number).await?;
        if pr.state == PrState::Merged {
            return Err(AppError::conflict(DOMAIN_CODE, "已合并的 PR 不可编辑"));
        }
        if pr.author != actor.id {
            self.ensure(
                actor,
                Action::Write,
                RepoId::from_uuid(self.repo_or_404(full_name).await?.id.as_uuid()),
            )
            .await?;
        }
        let updated = self.prs.update_edit(pr.id, edit).await?;
        self.publish("pr.updated", &updated, json!({"actor": actor.id.as_uuid()}))
            .await?;
        Ok(updated)
    }

    /// 关闭/重开。
    ///
    /// # Errors
    /// 不存在 `CR-404`;状态非法 `CR-409`;无权限 `CR-403`。
    pub async fn transition(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        target: PrState,
    ) -> Result<PullRequest, AppError> {
        let pr = self.get(actor, full_name, number).await?;
        if pr.author != actor.id {
            self.ensure(
                actor,
                Action::Write,
                RepoId::from_uuid(self.repo_or_404(full_name).await?.id.as_uuid()),
            )
            .await?;
        }
        let updated = match (pr.state, target) {
            (PrState::Merged, _) | (_, PrState::Merged) => {
                return Err(AppError::conflict(DOMAIN_CODE, "已合并的 PR 不可变更"));
            }
            (PrState::Open, PrState::Closed) | (PrState::Closed, PrState::Open) => {
                self.prs.set_state(pr.id, target).await?
            }
            (PrState::Open, PrState::Open) | (PrState::Closed, PrState::Closed) => pr,
        };
        let event = if target == PrState::Closed {
            "pr.closed"
        } else {
            "pr.reopened"
        };
        self.publish(event, &updated, json!({"actor": actor.id.as_uuid()}))
            .await?;
        Ok(updated)
    }

    /// 执行合并。
    ///
    /// # Errors
    /// 不存在 `CR-404`;Draft/终态/无权限 `CR-403,409`;冲突 `CR-409`;git 故障 `CR-500`。
    pub async fn merge(
        &self,
        actor: &CurrentUser,
        full_name: &str,
        number: i64,
        method: MergeMethod,
        message_title: Option<String>,
        message_body: Option<String>,
    ) -> Result<PullRequest, AppError> {
        let repo = self.repo_or_404(full_name).await?;
        if repo.archived {
            return Err(AppError::forbidden(DOMAIN_CODE, "仓库已归档"));
        }
        let pr = self.get(actor, full_name, number).await?;
        match pr.state {
            PrState::Merged => return Err(AppError::conflict(DOMAIN_CODE, "PR 已合并")),
            PrState::Closed => return Err(AppError::conflict(DOMAIN_CODE, "PR 已关闭")),
            PrState::Open => {}
        }
        if pr.is_draft {
            return Err(AppError::conflict(DOMAIN_CODE, "Draft PR 不可合并"));
        }
        self.ensure(actor, Action::Write, RepoId::from_uuid(repo.id.as_uuid()))
            .await?;
        // 分支保护门禁(T-205):评审聚合 + 状态检查聚合
        let repo_id = RepoId::from_uuid(repo.id.as_uuid());
        if let Some(rule) = self
            .protections
            .find_matching(repo_id, &pr.target_branch)
            .await?
        {
            let violations = self.gate_violations(&rule, &pr, full_name, repo_id).await?;
            if !violations.is_empty() {
                return Err(AppError::conflict(DOMAIN_CODE, "分支保护规则未满足")
                    .with_details(json!({"violations": violations})));
            }
        }

        let title = message_title.unwrap_or_else(|| pr.title.clone());
        let body = message_body.unwrap_or_else(|| pr.body.clone());
        let outcome = self
            .git
            .merge_branch(
                full_name,
                &MergeSpec {
                    target_branch: pr.target_branch.clone(),
                    head_branch: pr.source_branch.clone(),
                    method,
                    message_title: title.clone(),
                    message_body: body.clone(),
                    committer_name: actor.display_name.clone(),
                    committer_email: actor.email.clone(),
                },
            )
            .await
            .map_err(apperr)?;

        let (method_str, commit) = match outcome {
            codeplan_kernel::git::MergeOutcome::Merged(sha) => (method.as_str(), sha),
            codeplan_kernel::git::MergeOutcome::UpToDate => {
                return Err(AppError::conflict(DOMAIN_CODE, "源分支没有可合并的新提交"));
            }
            codeplan_kernel::git::MergeOutcome::Conflict(files) => {
                return Err(AppError::conflict(DOMAIN_CODE, "存在冲突")
                    .with_details(json!({"conflicts": files})));
            }
        };

        let merged = self
            .prs
            .mark_merged(pr.id, method_str, &commit, actor.id)
            .await?;
        self.publish(
            "pr.merged",
            &merged,
            json!({
                "repo_id": repo.id.as_uuid(),
                "full_name": full_name,
                "merge_commit": commit,
                "body": pr.body,
                "merged_to_default": pr.target_branch == repo.default_branch,
                "actor": actor.id.as_uuid(),
            }),
        )
        .await?;
        Ok(merged)
    }

    /// 计算规则下的门禁违规列表(空 = 放行)。
    async fn gate_violations(
        &self,
        rule: &crate::protection::BranchProtection,
        pr: &PullRequest,
        full_name: &str,
        repo_id: RepoId,
    ) -> Result<Vec<String>, AppError> {
        let mut failures = Vec::new();
        if rule.min_approvals > 0 {
            let reviews = self.reviews.list_by_pr(pr.id).await?;
            let summary = crate::review::aggregate_reviews(&reviews);
            if summary.combined != "approved" {
                failures.push(format!("评审门禁未通过(当前:{})", summary.combined));
            } else if (summary.approvals.len() as i32) < rule.min_approvals {
                failures.push(format!(
                    "通过人数不足:{}/{}",
                    summary.approvals.len(),
                    rule.min_approvals
                ));
            }
        }
        if !rule.required_contexts.is_empty() {
            let head = self
                .git
                .resolve_ref(full_name, &pr.source_branch)
                .await
                .map_err(apperr)?
                .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "源分支不存在"))?;
            let checks = self.statuses.list_by_commit(repo_id, &head).await?;
            for ctx in &rule.required_contexts {
                match checks.iter().find(|c| &c.context == ctx) {
                    None => failures.push(format!("缺少状态检查:{ctx}")),
                    Some(c) if c.state != CheckState::Success => failures.push(format!(
                        "状态检查未通过:{}({})",
                        c.context,
                        c.state.as_str()
                    )),
                    _ => {}
                }
            }
        }
        if rule.require_code_owner {
            let violations = self.owner_coverage_violations(pr, full_name).await?;
            failures.extend(violations);
        }
        Ok(failures)
    }

    /// CODEOWNERS 缺员检查:每个有所有者的变更文件,至少一名所有者通过。
    async fn owner_coverage_violations(
        &self,
        pr: &PullRequest,
        full_name: &str,
    ) -> Result<Vec<String>, AppError> {
        let (_, rules) = self.load_codeowners(full_name, &pr.target_branch).await?;
        if rules.is_empty() {
            return Ok(Vec::new());
        }
        let patch = self
            .git
            .diff_patch(full_name, &pr.target_branch, &pr.source_branch)
            .await
            .map_err(apperr)?;
        let files = diff_files_pub(&patch);
        let covered = owners_by_files(&rules, &files);
        if covered.is_empty() {
            return Ok(Vec::new());
        }
        let reviews = self.reviews.list_by_pr(pr.id).await?;
        let approved: std::collections::HashSet<RepoId> = std::collections::HashSet::new();
        let _ = approved;
        let mut latest: std::collections::HashMap<
            codeplan_kernel::ids::UserId,
            crate::review::Verdict,
        > = std::collections::HashMap::new();
        for r in &reviews {
            latest.entry(r.reviewer).or_insert(r.verdict);
        }
        let approved: std::collections::HashSet<codeplan_kernel::ids::UserId> = latest
            .into_iter()
            .filter(|(_, v)| *v == crate::review::Verdict::Approved)
            .map(|(u, _)| u)
            .collect();
        let mut failures = Vec::new();
        for (file, owners) in &covered {
            let mut resolved: Vec<codeplan_kernel::ids::UserId> = Vec::new();
            let mut names = Vec::new();
            for username in owners {
                names.push(format!("@{username}"));
                if let Some(u) = self.identity.by_username(username).await.map_err(apperr)? {
                    resolved.push(u.id);
                }
            }
            if resolved.is_empty() {
                // 所有者均不在平台:无法满足,视为缺员
                failures.push(format!(
                    "文件 {file} 的所有者均不在平台:{}",
                    names.join(" ")
                ));
                continue;
            }
            if !resolved.iter().any(|id| approved.contains(id)) {
                failures.push(format!("文件 {file} 缺少所有者评审:{}", names.join(" ")));
            }
        }
        Ok(failures)
    }

    async fn publish(
        &self,
        event_type: &str,
        pr: &PullRequest,
        mut extra: serde_json::Value,
    ) -> Result<(), AppError> {
        if let serde_json::Value::Object(map) = &mut extra {
            map.insert("pr_id".into(), json!(pr.id.as_uuid()));
            map.insert("number".into(), json!(pr.number));
            map.insert("title".into(), json!(pr.title));
            map.insert("source_branch".into(), json!(pr.source_branch));
            map.insert("target_branch".into(), json!(pr.target_branch));
            map.insert("state".into(), json!(pr.state.as_str()));
            map.insert("author".into(), json!(pr.author.as_uuid()));
        }
        self.events
            .publish(vec![codeplan_kernel::events::EventEnvelope::new(
                event_type, extra,
            )])
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e)))
    }
}

fn apperr(e: String) -> AppError {
    eprintln!("[pr] git 操作失败: {e}");
    AppError::internal(DOMAIN_CODE).with_details(json!(e))
}
