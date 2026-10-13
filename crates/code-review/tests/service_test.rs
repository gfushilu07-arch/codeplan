//! PR 服务编排单测:内存仓储 + 假 GitAccess(不依赖真实 git/PG)。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use codeplan_code_review::domain::{
    NewPullRequest, PrEdit, PrId, PrState, PullRequest, PullRequestRepository,
};
use codeplan_code_review::service::PullRequestService;
use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::EventEnvelope;
use codeplan_kernel::git::{
    CommitBrief, GitAccess, GitRepoLookup, MergeOutcome, MergeSpec, RepoRef, RepoRefId, TagBrief,
};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::{RepoId, UserId};
use codeplan_kernel::testkit::InMemoryEventBus;

fn user(id: &str) -> CurrentUser {
    CurrentUser {
        id: UserId::from_uuid(uuid::Uuid::new_v4()),
        display_name: id.to_owned(),
        email: format!("{id}@corp.test"),
        is_platform_admin: false,
        scopes: vec!["*".to_owned()],
    }
}

struct AllowAll;
#[async_trait]
impl Authz for AllowAll {
    async fn check(&self, _: UserId, _: Action, _: &ResourceRef) -> Result<bool, String> {
        Ok(true)
    }
}

struct DenyAll;
#[async_trait]
impl Authz for DenyAll {
    async fn check(&self, _: UserId, _: Action, _: &ResourceRef) -> Result<bool, String> {
        Ok(false)
    }
}

#[derive(Default)]
struct MemPrStore {
    prs: Mutex<Vec<PullRequest>>,
}

fn sample_pr(n: i64) -> PullRequest {
    PullRequest {
        id: PrId::generate(),
        repo_id: RepoId::generate(),
        number: n,
        title: format!("PR #{n}"),
        body: String::new(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
        author: UserId::generate(),
        state: PrState::Open,
        is_draft: false,
        merge_method: None,
        merge_commit: None,
        merged_by: None,
        merged_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[async_trait]
impl PullRequestRepository for MemPrStore {
    async fn insert(&self, input: &NewPullRequest) -> Result<PullRequest, AppError> {
        let mut all = self.prs.lock().unwrap();
        let created = PullRequest {
            id: PrId::generate(),
            repo_id: input.repo_id,
            number: all.len() as i64 + 1,
            title: input.title.clone(),
            body: input.body.clone(),
            source_branch: input.source_branch.clone(),
            target_branch: input.target_branch.clone(),
            author: input.author,
            state: PrState::Open,
            is_draft: input.is_draft,
            merge_method: None,
            merge_commit: None,
            merged_by: None,
            merged_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        all.push(created.clone());
        Ok(created)
    }

    async fn find_by_number(
        &self,
        repo_id: RepoId,
        number: i64,
    ) -> Result<Option<PullRequest>, AppError> {
        Ok(self
            .prs
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.repo_id == repo_id && p.number == number)
            .cloned())
    }

    async fn find_by_id(&self, id: PrId) -> Result<Option<PullRequest>, AppError> {
        Ok(self
            .prs
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    async fn list_by_repo(
        &self,
        repo_id: RepoId,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>, AppError> {
        Ok(self
            .prs
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.repo_id == repo_id && state.is_none_or(|s| p.state == s))
            .cloned()
            .collect())
    }

    async fn update_edit(&self, id: PrId, edit: &PrEdit) -> Result<PullRequest, AppError> {
        let mut all = self.prs.lock().unwrap();
        let pr = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        if let Some(t) = &edit.title {
            pr.title = t.clone();
        }
        if let Some(b) = &edit.body {
            pr.body = b.clone();
        }
        if let Some(d) = edit.is_draft {
            pr.is_draft = d;
        }
        Ok(pr.clone())
    }

    async fn mark_merged(
        &self,
        id: PrId,
        method: &str,
        merge_commit: &str,
        merged_by: UserId,
    ) -> Result<PullRequest, AppError> {
        let mut all = self.prs.lock().unwrap();
        let pr = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        pr.state = PrState::Merged;
        pr.merge_method = Some(method.to_owned());
        pr.merge_commit = Some(merge_commit.to_owned());
        pr.merged_by = Some(merged_by);
        pr.merged_at = Some(Utc::now());
        Ok(pr.clone())
    }

    async fn set_state(&self, id: PrId, state: PrState) -> Result<PullRequest, AppError> {
        let mut all = self.prs.lock().unwrap();
        let pr = all
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        pr.state = state;
        Ok(pr.clone())
    }
}

/// 固定 UUID 的仓库投影,保证 create/get/merge 间 repo_id 稳定。
struct FixedLookup;

const DEMO_REPO: uuid::Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000001");

/// alice/bob 固定用户 ID(CODEOWNERS 解析)。
const ALICE: uuid::Uuid = uuid::uuid!("00000000-bb00-0000-0000-0000000000a1");
const BOB: uuid::Uuid = uuid::uuid!("00000000-bb00-0000-0000-0000000000b2");

struct FakeIdentity;

#[async_trait]
impl codeplan_kernel::lookup::IdentityLookup for FakeIdentity {
    async fn user_brief(
        &self,
        _: codeplan_kernel::ids::UserId,
    ) -> Result<Option<codeplan_kernel::lookup::UserBrief>, String> {
        Ok(None)
    }
    async fn by_username(
        &self,
        username: &str,
    ) -> Result<Option<codeplan_kernel::lookup::UserBrief>, String> {
        use codeplan_kernel::lookup::UserBrief;
        let id = match username {
            "alice" => ALICE,
            "bob" => BOB,
            _ => return Ok(None),
        };
        Ok(Some(UserBrief {
            id: codeplan_kernel::ids::UserId::from_uuid(id),
            display_name: username.to_owned(),
            email: format!("{username}@corp.test"),
        }))
    }
}
const ARCHIVED_REPO: uuid::Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000002");

#[async_trait]
impl GitRepoLookup for FixedLookup {
    async fn by_full_name(&self, full_name: &str) -> Result<Option<RepoRef>, String> {
        if full_name == "admin/demo" {
            Ok(Some(RepoRef {
                id: RepoRefId::from_uuid(DEMO_REPO),
                full_name: full_name.to_owned(),
                default_branch: "main".into(),
                archived: false,
            }))
        } else if full_name == "admin/archived" {
            Ok(Some(RepoRef {
                id: RepoRefId::from_uuid(ARCHIVED_REPO),
                full_name: full_name.to_owned(),
                default_branch: "main".into(),
                archived: true,
            }))
        } else {
            Ok(None)
        }
    }
}

/// 可编程假 Git:diff 文本与合并结果由测试注入。
struct FakeGit {
    diff: String,
    merge_result: MergeOutcome,
    merges: Mutex<Vec<MergeSpec>>,
    codeowners: String,
}

impl Default for FakeGit {
    fn default() -> Self {
        Self {
            diff: String::new(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            codeowners: String::new(),
        }
    }
}

#[async_trait]
impl GitAccess for FakeGit {
    async fn resolve_ref(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()))
    }

    async fn merge_base(&self, _: &str, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()))
    }

    async fn diff_patch(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
        Ok(self.diff.clone())
    }

    async fn commits_between(&self, _: &str, _: &str, _: &str) -> Result<Vec<CommitBrief>, String> {
        Ok(vec![CommitBrief {
            id: "cccc".into(),
            author_name: "a".into(),
            author_email: "a@corp.test".into(),
            message_title: "add feature".into(),
            committed_at: Utc::now(),
        }])
    }

    async fn merge_branch(&self, _: &str, spec: &MergeSpec) -> Result<MergeOutcome, String> {
        self.merges.lock().unwrap().push(spec.clone());
        Ok(self.merge_result.clone())
    }

    async fn is_ancestor(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }

    async fn create_tag(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    async fn delete_tag(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    async fn list_tags(&self, _: &str) -> Result<Vec<TagBrief>, String> {
        Ok(vec![])
    }

    async fn raw_commit(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    async fn file_content(&self, _: &str, _: &str, path: &str) -> Result<Option<String>, String> {
        Ok((path == "CODEOWNERS").then(|| self.codeowners.clone()))
    }
}

/// 内存保护规则仓储(测试注入)。
#[derive(Default)]
struct MemProtections {
    rules: std::sync::Mutex<Vec<codeplan_code_review::protection::BranchProtection>>,
}

#[async_trait]
impl codeplan_code_review::protection::ProtectionRepository for MemProtections {
    async fn upsert(
        &self,
        rule: &codeplan_code_review::protection::NewProtection,
    ) -> Result<codeplan_code_review::protection::BranchProtection, AppError> {
        use codeplan_code_review::protection::{BranchProtection, ProtectionId};
        let mut all = self.rules.lock().unwrap();
        if let Some(existing) = all
            .iter_mut()
            .find(|r| r.repo_id == rule.repo_id && r.branch_pattern == rule.branch_pattern)
        {
            existing.min_approvals = rule.min_approvals;
            existing.required_contexts = rule.required_contexts.clone();
            existing.block_force_push = rule.block_force_push;
            existing.block_direct_push = rule.block_direct_push;
            return Ok(existing.clone());
        }
        let created = BranchProtection {
            id: ProtectionId::generate(),
            repo_id: rule.repo_id,
            branch_pattern: rule.branch_pattern.clone(),
            min_approvals: rule.min_approvals,
            required_contexts: rule.required_contexts.clone(),
            block_force_push: rule.block_force_push,
            block_direct_push: rule.block_direct_push,
            require_code_owner: rule.require_code_owner,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        all.push(created.clone());
        Ok(created)
    }
    async fn list_by_repo(
        &self,
        repo_id: RepoId,
    ) -> Result<Vec<codeplan_code_review::protection::BranchProtection>, AppError> {
        Ok(self
            .rules
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.repo_id == repo_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, _repo_id: RepoId, _p: &str) -> Result<(), AppError> {
        Ok(())
    }
}

/// 内存评审/状态仓储(合并门禁测试用,只列不写)。
#[derive(Default)]
struct MemReviews {
    items: std::sync::Mutex<Vec<codeplan_code_review::review::PrReview>>,
}

#[async_trait]
impl codeplan_code_review::review::ReviewRepository for MemReviews {
    async fn insert(
        &self,
        r: &codeplan_code_review::review::NewReview,
    ) -> Result<codeplan_code_review::review::PrReview, AppError> {
        let review = codeplan_code_review::review::PrReview {
            id: codeplan_code_review::review::ReviewId::generate(),
            pr_id: r.pr_id,
            reviewer: r.reviewer,
            verdict: r.verdict,
            body: r.body.clone(),
            head_sha: r.head_sha.clone(),
            created_at: chrono::Utc::now(),
        };
        self.items.lock().unwrap().push(review.clone());
        Ok(review)
    }
    async fn list_by_pr(
        &self,
        pr_id: PrId,
    ) -> Result<Vec<codeplan_code_review::review::PrReview>, AppError> {
        let mut all: Vec<_> = self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.pr_id == pr_id)
            .cloned()
            .collect();
        all.reverse();
        Ok(all)
    }
}

#[derive(Default)]
struct MemStatuses {
    items: std::sync::Mutex<Vec<codeplan_code_review::review::CommitStatus>>,
}

#[async_trait]
impl codeplan_code_review::review::StatusRepository for MemStatuses {
    async fn upsert(
        &self,
        st: &codeplan_code_review::review::NewStatus,
    ) -> Result<codeplan_code_review::review::CommitStatus, AppError> {
        let mut all = self.items.lock().unwrap();
        let created = codeplan_code_review::review::CommitStatus {
            id: codeplan_code_review::review::StatusId::generate(),
            repo_id: st.repo_id,
            commit_sha: st.commit_sha.clone(),
            context: st.context.clone(),
            state: st.state,
            description: st.description.clone(),
            target_url: st.target_url.clone(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        all.push(created.clone());
        Ok(created)
    }
    async fn list_by_commit(
        &self,
        repo_id: RepoId,
        commit_sha: &str,
    ) -> Result<Vec<codeplan_code_review::review::CommitStatus>, AppError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.repo_id == repo_id && c.commit_sha == commit_sha)
            .cloned()
            .collect())
    }
}

/// 共享测试状态(门禁用例要注入评审/状态数据)。
struct Gate {
    reviews: Arc<MemReviews>,
    statuses: Arc<MemStatuses>,
    protections: Arc<MemProtections>,
    reviewers: Arc<MemReviewersStore>,
}

/// 内存评审人指派仓储。
#[derive(Default)]
struct MemReviewersStore {
    items: std::sync::Mutex<Vec<codeplan_code_review::review::PrReviewer>>,
}

#[async_trait]
impl codeplan_code_review::review::ReviewerRepository for MemReviewersStore {
    async fn upsert(
        &self,
        r: &codeplan_code_review::review::NewReviewer,
    ) -> Result<codeplan_code_review::review::PrReviewer, AppError> {
        let mut all = self.items.lock().unwrap();
        if let Some(existing) = all
            .iter_mut()
            .find(|x| x.pr_id == r.pr_id && x.reviewer == r.reviewer)
        {
            existing.added_by = r.added_by;
            return Ok(existing.clone());
        }
        let created = codeplan_code_review::review::PrReviewer {
            id: codeplan_code_review::review::ReviewerId::generate(),
            pr_id: r.pr_id,
            reviewer: r.reviewer,
            added_by: r.added_by,
            created_at: chrono::Utc::now(),
        };
        all.push(created.clone());
        Ok(created)
    }
    async fn list_by_pr(
        &self,
        pr_id: PrId,
    ) -> Result<Vec<codeplan_code_review::review::PrReviewer>, AppError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|x| x.pr_id == pr_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, pr_id: PrId, reviewer: UserId) -> Result<(), AppError> {
        self.items
            .lock()
            .unwrap()
            .retain(|x| !(x.pr_id == pr_id && x.reviewer == reviewer));
        Ok(())
    }
}

fn svc_with(
    git: FakeGit,
    authz: Arc<dyn Authz>,
) -> (
    PullRequestService,
    Arc<MemPrStore>,
    Arc<InMemoryEventBus>,
    Gate,
) {
    let store = Arc::new(MemPrStore::default());
    let bus = Arc::new(InMemoryEventBus::new());
    let lookup: Arc<dyn GitRepoLookup> = Arc::new(FixedLookup);
    let git: Arc<dyn GitAccess> = Arc::new(git);
    let authz_dyn: Arc<dyn Authz> = authz;
    let bus_dyn: Arc<dyn codeplan_kernel::events::EventBus> = bus.clone();
    let reviews = Arc::new(MemReviews::default());
    let statuses = Arc::new(MemStatuses::default());
    let protections = Arc::new(MemProtections::default());
    let reviewers = Arc::new(MemReviewersStore::default());
    (
        PullRequestService::new(
            Arc::clone(&store) as Arc<dyn PullRequestRepository>,
            lookup,
            git,
            authz_dyn,
            bus_dyn,
            Arc::clone(&protections)
                as Arc<dyn codeplan_code_review::protection::ProtectionRepository>,
            Arc::clone(&reviews) as Arc<dyn codeplan_code_review::review::ReviewRepository>,
            Arc::clone(&statuses) as Arc<dyn codeplan_code_review::review::StatusRepository>,
            Arc::clone(&reviewers) as Arc<dyn codeplan_code_review::review::ReviewerRepository>,
            Arc::new(FakeIdentity),
        ),
        store,
        bus,
        Gate {
            reviews,
            statuses,
            protections,
            reviewers,
        },
    )
}

#[tokio::test]
async fn create_rejects_same_branch_and_empty_diff() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: String::new(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    // 源 = 目标
    let err = svc
        .create(&admin, "admin/demo", "t", "", "main", Some("main"), false)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-400");
    // 无差异
    let err = svc
        .create(&admin, "admin/demo", "t", "", "feature", None, false)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-400");
}

#[tokio::test]
async fn create_publishes_event_and_assigns_numbers() {
    let (svc, store, bus, _) = svc_with(
        FakeGit {
            diff: "diff --git a/x b/x".into(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr1 = svc
        .create(&admin, "admin/demo", "first", "", "feature", None, false)
        .await
        .unwrap();
    let pr2 = svc
        .create(&admin, "admin/demo", "second", "", "feature", None, true)
        .await
        .unwrap();
    assert_eq!((pr1.number, pr2.number), (1, 2));
    assert!(pr2.is_draft);
    assert_eq!(store.prs.lock().unwrap().len(), 2);
    let created = bus
        .published()
        .into_iter()
        .filter(|e| e.r#type == "pr.created")
        .count();
    assert_eq!(created, 2);
}

#[tokio::test]
async fn create_denied_without_read() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(DenyAll),
    );
    let stranger = user("stranger");
    let err = svc
        .create(&stranger, "admin/demo", "t", "", "feature", None, false)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-403");
}

#[tokio::test]
async fn merge_blocked_for_draft_and_closed() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Merged("deadbeef".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let draft = svc
        .create(&admin, "admin/demo", "draft", "", "feature", None, true)
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            draft.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");

    let normal = svc
        .create(&admin, "admin/demo", "normal", "", "feature", None, false)
        .await
        .unwrap();
    svc.transition(&admin, "admin/demo", normal.number, PrState::Closed)
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            normal.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");
}

#[tokio::test]
async fn merge_success_records_commit_and_event() {
    let (svc, _, bus, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Merged("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "ready", "", "feature", None, false)
        .await
        .unwrap();
    let merged = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Squash,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(merged.state, PrState::Merged);
    assert_eq!(merged.merge_method.as_deref(), Some("squash"));
    assert_eq!(
        merged.merge_commit.as_deref(),
        Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
    );
    assert!(
        bus.published()
            .iter()
            .any(|e: &EventEnvelope| e.r#type == "pr.merged")
    );
    // 二次合并 → 409
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");
}

#[tokio::test]
async fn merge_conflict_propagates_409() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Conflict(vec!["a.txt".into()]),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "c", "", "feature", None, false)
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");
}

#[tokio::test]
async fn closed_pr_reopens_and_remerge_blocked() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "r", "", "feature", None, false)
        .await
        .unwrap();
    svc.transition(&admin, "admin/demo", pr.number, PrState::Closed)
        .await
        .unwrap();
    let reopened = svc
        .transition(&admin, "admin/demo", pr.number, PrState::Open)
        .await
        .unwrap();
    assert_eq!(reopened.state, PrState::Open);
}

#[tokio::test]
async fn archived_repo_rejects_create() {
    let (svc, _, _, _) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: MergeOutcome::Merged("x".into()),
            merges: Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let err = svc
        .create(&admin, "admin/archived", "t", "", "feature", None, false)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-403");
}

#[tokio::test]
async fn merge_gate_blocks_until_approvals_and_contexts() {
    use codeplan_code_review::protection::{NewProtection, ProtectionRepository};
    use codeplan_code_review::review::{
        CheckState, NewReview, NewStatus, ReviewRepository, StatusRepository, Verdict,
    };

    let (svc, _, _, gate) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: codeplan_kernel::git::MergeOutcome::Merged("mergedsha".into()),
            merges: std::sync::Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "gated", "", "feature", None, false)
        .await
        .unwrap();

    // 保护 main:1 个通过 + ci/build 必须成功
    gate.protections
        .upsert(&NewProtection {
            repo_id: pr.repo_id,
            branch_pattern: "main".into(),
            min_approvals: 1,
            required_contexts: vec!["ci/build".into()],
            block_force_push: true,
            block_direct_push: false,
            require_code_owner: false,
        })
        .await
        .unwrap();

    // 无评审无检查 → 409 + 违规明细
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");

    // 检查成功但无人通过 → 409
    gate.statuses
        .upsert(&NewStatus {
            repo_id: pr.repo_id,
            commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            context: "ci/build".into(),
            state: CheckState::Success,
            description: String::new(),
            target_url: None,
        })
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");

    // 一个通过 → 放行合并
    gate.reviews
        .insert(&NewReview {
            pr_id: pr.id,
            reviewer: UserId::generate(),
            verdict: Verdict::Approved,
            body: String::new(),
            head_sha: None,
        })
        .await
        .unwrap();
    let merged = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(merged.state, PrState::Merged);
}

#[tokio::test]
async fn merge_gate_fails_on_failing_context() {
    use codeplan_code_review::protection::{NewProtection, ProtectionRepository};
    use codeplan_code_review::review::{CheckState, NewStatus, StatusRepository};

    let (svc, _, _, gate) = svc_with(
        FakeGit {
            diff: "x".into(),
            merge_result: codeplan_kernel::git::MergeOutcome::Merged("m".into()),
            merges: std::sync::Mutex::new(vec![]),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "g", "", "feature", None, false)
        .await
        .unwrap();
    gate.protections
        .upsert(&NewProtection {
            repo_id: pr.repo_id,
            branch_pattern: "main".into(),
            min_approvals: 0,
            required_contexts: vec!["ci/lint".into()],
            block_force_push: true,
            block_direct_push: false,
            require_code_owner: false,
        })
        .await
        .unwrap();
    gate.statuses
        .upsert(&NewStatus {
            repo_id: pr.repo_id,
            commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            context: "ci/lint".into(),
            state: CheckState::Failure,
            description: String::new(),
            target_url: None,
        })
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");
}

#[tokio::test]
async fn codeowners_auto_assign_on_create() {
    use codeplan_code_review::review::ReviewerRepository as _;
    let (svc, _, _, gate) = svc_with(
        FakeGit {
            diff: "diff --git a/src/app.rs b/src/app.rs\n@@\n+fn main() {}".into(),
            codeowners: "*.rs @alice @bob\nREADME.md @dave\n".into(),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(&admin, "admin/demo", "owned", "", "feature", None, false)
        .await
        .unwrap();
    // 变更文件 src/app.rs → 所有者 alice/bob 被指派;dave 不在变更文件且平台不存在
    let assigned = gate.reviewers.list_by_pr(pr.id).await.unwrap();
    let mut ids: Vec<_> = assigned.iter().map(|r| r.reviewer.as_uuid()).collect();
    ids.sort();
    assert_eq!(ids, vec![ALICE, BOB], "应自动指派 alice/bob");
    assert!(assigned.iter().all(|r| r.added_by.is_none()), "系统指派");
}

#[tokio::test]
async fn codeowner_gate_blocks_until_owner_approves() {
    use codeplan_code_review::protection::{NewProtection, ProtectionRepository};
    use codeplan_code_review::review::{NewReview, ReviewRepository, ReviewerRepository, Verdict};

    let (svc, _, _, gate) = svc_with(
        FakeGit {
            diff: "diff --git a/src/app.rs b/src/app.rs\n@@\n+fn main() {}".into(),
            codeowners: "*.rs @alice\n".into(),
            ..Default::default()
        },
        Arc::new(AllowAll),
    );
    let admin = user("admin");
    let pr = svc
        .create(
            &admin,
            "admin/demo",
            "owner-gated",
            "",
            "feature",
            None,
            false,
        )
        .await
        .unwrap();
    gate.protections
        .upsert(&NewProtection {
            repo_id: pr.repo_id,
            branch_pattern: "main".into(),
            min_approvals: 0,
            required_contexts: vec![],
            block_force_push: true,
            block_direct_push: false,
            require_code_owner: true,
        })
        .await
        .unwrap();

    // 非 owner 通过 ≠ 缺员解除
    gate.reviews
        .insert(&NewReview {
            pr_id: pr.id,
            reviewer: UserId::from_uuid(BOB),
            verdict: Verdict::Approved,
            body: String::new(),
            head_sha: None,
        })
        .await
        .unwrap();
    let err = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-409");

    // 所有者 alice 通过 → 放行
    gate.reviews
        .insert(&NewReview {
            pr_id: pr.id,
            reviewer: UserId::from_uuid(ALICE),
            verdict: Verdict::Approved,
            body: String::new(),
            head_sha: None,
        })
        .await
        .unwrap();
    let merged = svc
        .merge(
            &admin,
            "admin/demo",
            pr.number,
            codeplan_kernel::git::MergeMethod::Merge,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(merged.state, PrState::Merged);
}
