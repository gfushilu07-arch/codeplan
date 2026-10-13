//! 评审流单测(T-204):评审规则、聚合、锚定校验、状态检查(内存仓储 + 假 Git)。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use codeplan_code_review::domain::{
    NewPullRequest, PrEdit, PrId, PrState, PullRequest, PullRequestRepository,
};
use codeplan_code_review::protection::{
    BranchProtection, NewProtection, ProtectionId, ProtectionRepository,
};
use codeplan_code_review::review::aggregate_reviews;
use codeplan_code_review::review::{
    CheckState, CommentId, CommentRepository, CommitStatus, InlineAnchor, NewComment, NewReview,
    NewStatus, PrComment, PrReview, ReviewId, ReviewRepository, Side, StatusId, StatusRepository,
    StatusSummary, Verdict,
};
use codeplan_code_review::review_service::{ReviewService, diff_files};
use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::events::EventBus;
use codeplan_kernel::git::{
    CommitBrief, GitAccess, GitRepoLookup, MergeOutcome, MergeSpec, RepoRef, RepoRefId, TagBrief,
};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::{RepoId, UserId};
use codeplan_kernel::testkit::InMemoryEventBus;

// ---------- 通用替身 ----------

fn user(name: &str) -> CurrentUser {
    CurrentUser {
        id: UserId::from_uuid(match name {
            "author" => uuid::uuid!("00000000-aa00-0000-0000-0000000000a0"),
            "reviewer" => uuid::uuid!("00000000-aa00-0000-0000-0000000000b0"),
            "reviewer2" => uuid::uuid!("00000000-aa00-0000-0000-0000000000c0"),
            _ => UserId::generate().as_uuid(),
        }),
        display_name: name.to_owned(),
        email: format!("{name}@corp.test"),
        is_platform_admin: false,
        scopes: vec!["*".to_owned()],
    }
}

const DEMO_REPO: uuid::Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000001");

struct AllowAll;
#[async_trait]
impl Authz for AllowAll {
    async fn check(&self, _: UserId, _: Action, _: &ResourceRef) -> Result<bool, String> {
        Ok(true)
    }
}

/// 只放行 Read。
struct ReadOnly;
#[async_trait]
impl Authz for ReadOnly {
    async fn check(&self, _: UserId, action: Action, _: &ResourceRef) -> Result<bool, String> {
        Ok(matches!(action, Action::Read))
    }
}

struct FixedLookup;
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
        } else {
            Ok(None)
        }
    }
}

const DIFF: &str = "diff --git a/src/app.rs b/src/app.rs\nindex 111..222 100644\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/README.md b/README.md\nindex 333..444 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-hi\n+hello\n";

const FAKE_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

struct FakeGit;
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
        _: &str,
    ) -> Result<Option<codeplan_kernel::lookup::UserBrief>, String> {
        Ok(None)
    }
}

struct FakeIdentity;

#[async_trait]
impl GitAccess for FakeGit {
    async fn resolve_ref(&self, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(Some(FAKE_SHA.to_owned()))
    }
    async fn merge_base(&self, _: &str, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(Some("base".into()))
    }
    async fn diff_patch(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
        Ok(DIFF.into())
    }
    async fn commits_between(&self, _: &str, _: &str, _: &str) -> Result<Vec<CommitBrief>, String> {
        Ok(vec![])
    }
    async fn merge_branch(&self, _: &str, _: &MergeSpec) -> Result<MergeOutcome, String> {
        Ok(MergeOutcome::Merged("x".into()))
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

    async fn file_content(&self, _: &str, _: &str, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

// ---------- 内存仓储 ----------

#[derive(Default)]
struct MemPrs {
    prs: Mutex<Vec<PullRequest>>,
}

#[async_trait]
impl PullRequestRepository for MemPrs {
    async fn insert(&self, input: &NewPullRequest) -> Result<PullRequest, AppError> {
        let mut all = self.prs.lock().unwrap();
        let pr = PullRequest {
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
        all.push(pr.clone());
        Ok(pr)
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
        Ok(pr.clone())
    }
    async fn mark_merged(
        &self,
        _id: PrId,
        _m: &str,
        _c: &str,
        _by: UserId,
    ) -> Result<PullRequest, AppError> {
        Err(AppError::internal("CR"))
    }
    async fn set_state(&self, _id: PrId, _s: PrState) -> Result<PullRequest, AppError> {
        Err(AppError::internal("CR"))
    }
}

#[derive(Default)]
struct MemComments {
    items: Mutex<Vec<PrComment>>,
}

#[async_trait]
impl CommentRepository for MemComments {
    async fn insert(&self, c: &NewComment) -> Result<PrComment, AppError> {
        let comment = PrComment {
            id: CommentId::generate(),
            pr_id: c.pr_id,
            author: c.author,
            body: c.body.clone(),
            file_path: c.anchor.as_ref().map(|a| a.file_path.clone()),
            line: c.anchor.as_ref().map(|a| a.line),
            side: c.anchor.as_ref().map(|a| a.side),
            anchor_sha: c.anchor_sha.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.items.lock().unwrap().push(comment.clone());
        Ok(comment)
    }
    async fn list_by_pr(&self, pr_id: PrId) -> Result<Vec<PrComment>, AppError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.pr_id == pr_id)
            .cloned()
            .collect())
    }
    async fn delete(&self, id: CommentId) -> Result<(), AppError> {
        let mut items = self.items.lock().unwrap();
        let before = items.len();
        items.retain(|c| c.id != id);
        if items.len() == before {
            return Err(AppError::not_found("CR", "评论不存在"));
        }
        Ok(())
    }
}

#[derive(Default)]
struct MemReviews {
    items: Mutex<Vec<PrReview>>,
}

#[async_trait]
impl ReviewRepository for MemReviews {
    async fn insert(&self, r: &NewReview) -> Result<PrReview, AppError> {
        let review = PrReview {
            id: ReviewId::generate(),
            pr_id: r.pr_id,
            reviewer: r.reviewer,
            verdict: r.verdict,
            body: r.body.clone(),
            head_sha: r.head_sha.clone(),
            created_at: Utc::now(),
        };
        self.items.lock().unwrap().push(review.clone());
        Ok(review)
    }
    async fn list_by_pr(&self, pr_id: PrId) -> Result<Vec<PrReview>, AppError> {
        let mut all: Vec<PrReview> = self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.pr_id == pr_id)
            .cloned()
            .collect();
        all.reverse(); // 时间倒序
        Ok(all)
    }
}

#[derive(Default)]
struct MemStatuses {
    items: Mutex<Vec<CommitStatus>>,
}

#[async_trait]
impl StatusRepository for MemStatuses {
    async fn upsert(&self, s: &NewStatus) -> Result<CommitStatus, AppError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|c| {
            c.repo_id == s.repo_id && c.commit_sha == s.commit_sha && c.context == s.context
        }) {
            existing.state = s.state;
            existing.description = s.description.clone();
            existing.target_url = s.target_url.clone();
            return Ok(existing.clone());
        }
        let st = CommitStatus {
            id: StatusId::generate(),
            repo_id: s.repo_id,
            commit_sha: s.commit_sha.clone(),
            context: s.context.clone(),
            state: s.state,
            description: s.description.clone(),
            target_url: s.target_url.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        items.push(st.clone());
        Ok(st)
    }
    async fn list_by_commit(
        &self,
        repo_id: RepoId,
        commit_sha: &str,
    ) -> Result<Vec<CommitStatus>, AppError> {
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

#[derive(Default)]
struct MemProtections {
    rules: Mutex<Vec<BranchProtection>>,
}

#[async_trait]
impl ProtectionRepository for MemProtections {
    async fn upsert(&self, rule: &NewProtection) -> Result<BranchProtection, AppError> {
        let created = BranchProtection {
            id: ProtectionId::generate(),
            repo_id: rule.repo_id,
            branch_pattern: rule.branch_pattern.clone(),
            min_approvals: rule.min_approvals,
            required_contexts: rule.required_contexts.clone(),
            block_force_push: rule.block_force_push,
            block_direct_push: rule.block_direct_push,
            require_code_owner: rule.require_code_owner,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.rules.lock().unwrap().push(created.clone());
        Ok(created)
    }
    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<BranchProtection>, AppError> {
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

#[derive(Default)]
struct MemReviewers {
    items: Mutex<Vec<codeplan_code_review::review::PrReviewer>>,
}

#[async_trait]
impl codeplan_code_review::review::ReviewerRepository for MemReviewers {
    async fn upsert(
        &self,
        r: &codeplan_code_review::review::NewReviewer,
    ) -> Result<codeplan_code_review::review::PrReviewer, AppError> {
        let created = codeplan_code_review::review::PrReviewer {
            id: codeplan_code_review::review::ReviewerId::generate(),
            pr_id: r.pr_id,
            reviewer: r.reviewer,
            added_by: r.added_by,
            created_at: Utc::now(),
        };
        self.items.lock().unwrap().push(created.clone());
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
    async fn delete(
        &self,
        pr_id: PrId,
        reviewer: codeplan_kernel::ids::UserId,
    ) -> Result<(), AppError> {
        self.items
            .lock()
            .unwrap()
            .retain(|x| !(x.pr_id == pr_id && x.reviewer == reviewer));
        Ok(())
    }
}

// ---------- 装配 ----------

fn build(
    prs: Arc<MemPrs>,
    comments: Arc<MemComments>,
    reviews: Arc<MemReviews>,
    statuses: Arc<MemStatuses>,
    protections: Arc<MemProtections>,
    reviewers: Arc<MemReviewers>,
    authz: Arc<dyn Authz>,
    bus: Arc<InMemoryEventBus>,
) -> ReviewService {
    let lookup: Arc<dyn GitRepoLookup> = Arc::new(FixedLookup);
    let git: Arc<dyn GitAccess> = Arc::new(FakeGit);
    ReviewService::new(
        prs,
        comments,
        reviews,
        statuses,
        protections,
        reviewers,
        Arc::new(FakeIdentity),
        lookup,
        git,
        authz,
        bus,
    )
}

struct Fixtures {
    prs: Arc<MemPrs>,
    comments: Arc<MemComments>,
    reviews: Arc<MemReviews>,
    statuses: Arc<MemStatuses>,
    protections: Arc<MemProtections>,
    reviewers: Arc<MemReviewers>,
    bus: Arc<InMemoryEventBus>,
}

fn fixtures() -> Fixtures {
    Fixtures {
        prs: Arc::new(MemPrs::default()),
        comments: Arc::new(MemComments::default()),
        reviews: Arc::new(MemReviews::default()),
        statuses: Arc::new(MemStatuses::default()),
        protections: Arc::new(MemProtections::default()),
        reviewers: Arc::new(MemReviewers::default()),
        bus: Arc::new(InMemoryEventBus::new()),
    }
}

fn svc(fx: &Fixtures) -> ReviewService {
    build(
        Arc::clone(&fx.prs),
        Arc::clone(&fx.comments),
        Arc::clone(&fx.reviews),
        Arc::clone(&fx.statuses),
        Arc::clone(&fx.protections),
        Arc::clone(&fx.reviewers),
        Arc::new(AllowAll),
        Arc::clone(&fx.bus),
    )
}

async fn open_pr(prs: &MemPrs, author: UserId) -> PullRequest {
    prs.insert(&NewPullRequest {
        repo_id: RepoId::from_uuid(DEMO_REPO),
        title: "test pr".into(),
        body: String::new(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
        author,
        is_draft: false,
    })
    .await
    .unwrap()
}

// ---------- 用例 ----------

#[test]
fn diff_files_extraction() {
    let files = diff_files(DIFF);
    assert_eq!(files, vec!["src/app.rs".to_owned(), "README.md".to_owned()]);
}

#[test]
fn aggregate_latest_verdict_wins() {
    let a = UserId::from_uuid(uuid::uuid!("00000000-aa00-0000-0000-0000000000b0"));
    let mk = |v: Verdict, sha: &str| PrReview {
        id: ReviewId::generate(),
        pr_id: PrId::generate(),
        reviewer: a,
        verdict: v,
        body: String::new(),
        head_sha: Some(sha.into()),
        created_at: Utc::now(),
    };
    // 输入按时间倒序:第一条是最新的
    let reviews = vec![
        mk(Verdict::ChangesRequested, "new"),
        mk(Verdict::Approved, "old"),
    ];
    let s = aggregate_reviews(&reviews);
    assert!(s.changes_requested.len() == 1 && s.approvals.is_empty());
    assert_eq!(s.combined, "changes_requested");

    let reviews = vec![mk(Verdict::Approved, "new")];
    assert_eq!(aggregate_reviews(&reviews).combined, "approved");

    assert_eq!(aggregate_reviews(&[]).combined, "pending");
}

#[test]
fn status_summary_combination() {
    let mk = |ctx: &str, st: CheckState| CommitStatus {
        id: StatusId::generate(),
        repo_id: RepoId::from_uuid(DEMO_REPO),
        commit_sha: "abc".into(),
        context: ctx.into(),
        state: st,
        description: String::new(),
        target_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    assert_eq!(StatusSummary::aggregate(vec![]).combined, "none");
    assert_eq!(
        StatusSummary::aggregate(vec![mk("ci/build", CheckState::Success)]).combined,
        "success"
    );
    assert_eq!(
        StatusSummary::aggregate(vec![
            mk("ci/build", CheckState::Success),
            mk("ci/lint", CheckState::Pending),
        ])
        .combined,
        "pending"
    );
    assert_eq!(
        StatusSummary::aggregate(vec![
            mk("ci/build", CheckState::Success),
            mk("ci/lint", CheckState::Failure),
        ])
        .combined,
        "failure"
    );
    // upsert 后同 context 覆盖(聚合输入不重复)
    assert_eq!(
        StatusSummary::aggregate(vec![mk("ci", CheckState::Error)]).combined,
        "failure"
    );
}

#[tokio::test]
async fn self_approve_blocked_and_other_ok() {
    let fx = fixtures();
    let svc = svc(&fx);
    let bus = Arc::clone(&fx.bus);
    let prs = &fx.prs;
    let author = user("author");
    let pr = open_pr(&prs, author.id).await;
    let reviewer = user("reviewer");

    let err = svc
        .post_review(&author, "admin/demo", pr.number, Verdict::Approved, "lgtm")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-400", "不能自评通过");

    let review = svc
        .post_review(
            &reviewer,
            "admin/demo",
            pr.number,
            Verdict::Approved,
            "lgtm",
        )
        .await
        .unwrap();
    assert_eq!(review.verdict, Verdict::Approved);
    assert_eq!(review.head_sha.as_deref(), Some(FAKE_SHA));

    // 驳回覆盖通过
    svc.post_review(
        &reviewer,
        "admin/demo",
        pr.number,
        Verdict::ChangesRequested,
        "fix",
    )
    .await
    .unwrap();
    let summary = svc
        .review_summary(&reviewer, "admin/demo", pr.number)
        .await
        .unwrap();
    assert_eq!(summary.combined, "changes_requested");
    assert!(
        bus.published()
            .iter()
            .any(|e| e.r#type == "pr.review_posted")
    );
}

#[tokio::test]
async fn author_comment_verdict_allowed() {
    let fx = fixtures();
    let svc = svc(&fx);
    let prs = &fx.prs;
    let author = user("author");
    let pr = open_pr(&prs, author.id).await;
    let r = svc
        .post_review(
            &author,
            "admin/demo",
            pr.number,
            Verdict::Comment,
            "说明一下",
        )
        .await
        .unwrap();
    assert_eq!(r.verdict, Verdict::Comment);
}

#[tokio::test]
async fn inline_comment_anchor_validation() {
    let fx = fixtures();
    let svc = svc(&fx);
    let bus = Arc::clone(&fx.bus);
    let prs = &fx.prs;
    let author = user("author");
    let pr = open_pr(&prs, author.id).await;

    // diff 内文件 OK
    let c = svc
        .add_comment(
            &author,
            "admin/demo",
            pr.number,
            "这里有 bug",
            Some(InlineAnchor {
                file_path: "src/app.rs".into(),
                line: 1,
                side: Side::Right,
            }),
        )
        .await
        .unwrap();
    assert_eq!(c.file_path.as_deref(), Some("src/app.rs"));
    assert_eq!(c.anchor_sha.as_deref(), Some(FAKE_SHA));

    // diff 外文件拒
    let err = svc
        .add_comment(
            &author,
            "admin/demo",
            pr.number,
            "x",
            Some(InlineAnchor {
                file_path: "other.txt".into(),
                line: 1,
                side: Side::Right,
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-400");

    // 行号非法
    let err = svc
        .add_comment(
            &author,
            "admin/demo",
            pr.number,
            "x",
            Some(InlineAnchor {
                file_path: "src/app.rs".into(),
                line: 0,
                side: Side::Right,
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-400");

    // 对话流评论
    svc.add_comment(&author, "admin/demo", pr.number, "整体 OK", None)
        .await
        .unwrap();
    let all = svc
        .list_comments(&author, "admin/demo", pr.number)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert!(
        bus.published()
            .iter()
            .any(|e| e.r#type == "pr.comment_created")
    );
}

#[tokio::test]
async fn comment_delete_permissions() {
    let fx = fixtures();
    let svc = svc(&fx);
    let prs = Arc::clone(&fx.prs);
    let author = user("author");
    let stranger = user("stranger");
    let pr = open_pr(&prs, author.id).await;
    let c = svc
        .add_comment(&author, "admin/demo", pr.number, "hi", None)
        .await
        .unwrap();

    // 非作者且无 Write → 403(共享存储的只读权限服务)
    let no_write = build(
        prs,
        Arc::clone(&fx.comments),
        Arc::clone(&fx.reviews),
        Arc::clone(&fx.statuses),
        Arc::clone(&fx.protections),
        Arc::clone(&fx.reviewers),
        Arc::new(ReadOnly),
        Arc::clone(&fx.bus),
    );
    let err = no_write
        .delete_comment(&stranger, "admin/demo", pr.number, c.id)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-403");

    // 作者本人可删
    svc.delete_comment(&author, "admin/demo", pr.number, c.id)
        .await
        .unwrap();
    assert_eq!(
        svc.list_comments(&author, "admin/demo", pr.number)
            .await
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn status_upsert_and_pr_aggregation() {
    let fx = fixtures();
    let svc = svc(&fx);
    let bus = Arc::clone(&fx.bus);
    let prs = &fx.prs;
    let author = user("author");
    let ci = user("ci-bot");
    let pr = open_pr(&prs, author.id).await;

    svc.set_status(
        &ci,
        "admin/demo",
        FAKE_SHA,
        "ci/build",
        CheckState::Pending,
        "running",
        None,
    )
    .await
    .unwrap();
    let s = svc
        .pr_status(&author, "admin/demo", pr.number)
        .await
        .unwrap();
    assert_eq!(s.combined, "pending");

    // 覆盖为 success + 新增 lint
    svc.set_status(
        &ci,
        "admin/demo",
        FAKE_SHA,
        "ci/build",
        CheckState::Success,
        "ok",
        None,
    )
    .await
    .unwrap();
    svc.set_status(
        &ci,
        "admin/demo",
        FAKE_SHA,
        "ci/lint",
        CheckState::Success,
        "ok",
        None,
    )
    .await
    .unwrap();
    let s = svc
        .pr_status(&author, "admin/demo", pr.number)
        .await
        .unwrap();
    assert_eq!(s.combined, "success");
    assert_eq!(s.checks.len(), 2);

    svc.set_status(
        &ci,
        "admin/demo",
        FAKE_SHA,
        "ci/lint",
        CheckState::Failure,
        "bad",
        None,
    )
    .await
    .unwrap();
    let s = svc
        .pr_status(&author, "admin/demo", pr.number)
        .await
        .unwrap();
    assert_eq!(s.combined, "failure");
    assert!(
        bus.published()
            .iter()
            .any(|e| e.r#type == "commit.status_updated")
    );
}

#[tokio::test]
async fn set_status_requires_write() {
    let fx = fixtures();
    let no_write = build(
        Arc::clone(&fx.prs),
        Arc::clone(&fx.comments),
        Arc::clone(&fx.reviews),
        Arc::clone(&fx.statuses),
        Arc::clone(&fx.protections),
        Arc::clone(&fx.reviewers),
        Arc::new(ReadOnly),
        Arc::clone(&fx.bus),
    );
    let err = no_write
        .set_status(
            &user("reader"),
            "admin/demo",
            "abc123",
            "ci/build",
            CheckState::Success,
            "",
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CR-403");
}
