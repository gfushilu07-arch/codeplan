//! 跨域 Git 访问契约(T-203):评审域等 L3 域消费,git-service 实现,server 装配注入。
//!
//! 依据 ARCHITECTURE.md §1.1 规则 2:L3 域间禁止互相依赖,跨域只有
//! 事件与 kernel trait 两条路;此处把"仓库元数据只读投影 + git 内容/合并
//! 操作"收敛为本契约。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;

define_id!(
    /// 仓库 ID(跨域引用投影)。
    RepoRefId
);

/// 仓库摘要(跨域只读投影,不含领域内部细节)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RepoRef {
    /// 仓库 ID。
    pub id: RepoRefId,
    /// 全名(`{ownerKey}/{key}`)。
    pub full_name: String,
    /// 默认分支。
    pub default_branch: String,
    /// 是否已归档(归档仓库禁止一切写操作)。
    pub archived: bool,
}

/// 提交摘要。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommitBrief {
    /// 提交 SHA。
    pub id: String,
    /// 作者名。
    pub author_name: String,
    /// 作者邮箱。
    pub author_email: String,
    /// 标题行。
    pub message_title: String,
    /// 提交时间。
    pub committed_at: DateTime<Utc>,
}

/// 仓库元数据只读查找。
#[async_trait::async_trait]
pub trait GitRepoLookup: Send + Sync {
    /// 按全名查仓库;不存在返回 None。
    ///
    /// # Errors
    /// 元数据源故障。
    async fn by_full_name(&self, full_name: &str) -> Result<Option<RepoRef>, String>;
}

/// 服务端合并方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    /// 产生双亲合并提交。
    Merge,
    /// 压缩为单亲提交(取源分支最终树)。
    Squash,
    /// 把源分支提交逐个重放到目标顶端(cherry-pick 语义)。
    Rebase,
}

impl MergeMethod {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// 合并执行参数。
#[derive(Debug, Clone)]
pub struct MergeSpec {
    /// 目标分支。
    pub target_branch: String,
    /// 源分支。
    pub head_branch: String,
    /// 合并方式。
    pub method: MergeMethod,
    /// 提交信息标题。
    pub message_title: String,
    /// 提交信息正文。
    pub message_body: String,
    /// 提交者名(合并操作人)。
    pub committer_name: String,
    /// 提交者邮箱。
    pub committer_email: String,
}

/// 合并结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// 合并成功:新提交 SHA(目标分支已前移)。
    Merged(String),
    /// 源分支内容已全部在目标中,无需合并。
    UpToDate,
    /// 内容冲突(冲突文件路径列表)。
    Conflict(Vec<String>),
}

/// Git 内容访问与合并执行(裸仓库服务端操作)。
#[async_trait::async_trait]
pub trait GitAccess: Send + Sync {
    /// 解析引用(分支名/SHA)到 commit SHA;不存在返回 None。
    ///
    /// # Errors
    /// git 执行故障。
    async fn resolve_ref(&self, full_name: &str, reference: &str)
    -> Result<Option<String>, String>;

    /// 两引用的 merge-base;无共同祖先返回 None。
    ///
    /// # Errors
    /// git 执行故障。
    async fn merge_base(&self, full_name: &str, a: &str, b: &str)
    -> Result<Option<String>, String>;

    /// 三点差异补丁(`merge-base(base, head)..head` 的 unified diff)。
    ///
    /// # Errors
    /// git 执行故障。
    async fn diff_patch(&self, full_name: &str, base: &str, head: &str) -> Result<String, String>;

    /// `merge-base(base, head)..head` 区间提交(时间正序)。
    ///
    /// # Errors
    /// git 执行故障。
    async fn commits_between(
        &self,
        full_name: &str,
        base: &str,
        head: &str,
    ) -> Result<Vec<CommitBrief>, String>;

    /// 执行合并并把目标分支前移到新提交(带旧值 CAS)。
    ///
    /// # Errors
    /// git 执行故障;分支不存在等。
    async fn merge_branch(&self, full_name: &str, spec: &MergeSpec)
    -> Result<MergeOutcome, String>;

    /// 创建附注标签(已存在报错)。
    ///
    /// # Errors
    /// git 执行故障;标签已存在。
    async fn create_tag(
        &self,
        full_name: &str,
        name: &str,
        target: &str,
        message: &str,
    ) -> Result<(), String>;

    /// 删除标签(不存在返回 Ok,幂等)。
    ///
    /// # Errors
    /// git 执行故障。
    async fn delete_tag(&self, full_name: &str, name: &str) -> Result<(), String>;

    /// 标签列表(按创建时间倒序)。
    ///
    /// # Errors
    /// git 执行故障。
    async fn list_tags(&self, full_name: &str) -> Result<Vec<TagBrief>, String>;

    /// 读取原始提交对象(gpgsig 验签用);不存在返回 None。
    ///
    /// # Errors
    /// git 执行故障。
    async fn raw_commit(&self, full_name: &str, sha: &str) -> Result<Option<String>, String>;

    /// 读取引用下某文件内容(CODEOWNERS 等);不存在返回 None。
    ///
    /// # Errors
    /// git 执行故障。
    async fn file_content(
        &self,
        full_name: &str,
        reference: &str,
        path: &str,
    ) -> Result<Option<String>, String>;

    /// `a` 是否为 `b` 的祖先(含相等);用于 force push 判定。
    ///
    /// # Errors
    /// git 执行故障。
    async fn is_ancestor(&self, full_name: &str, a: &str, b: &str) -> Result<bool, String>;
}

/// 标签摘要。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TagBrief {
    /// 标签名。
    pub name: String,
    /// 指向的提交 SHA。
    pub commit_sha: String,
    /// 创建时间(RFC 3339)。
    pub created_at: String,
}

/// 一次推送的引用变更(`old`/`new` 为提交 SHA;new 全零 = 删除)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRef {
    /// 引用全名(如 `refs/heads/main`)。
    pub ref_name: String,
    /// 旧 SHA(全零 = 新建)。
    pub old: String,
    /// 新 SHA(全零 = 删除)。
    pub new: String,
}

/// 推送门禁契约(T-205):分支保护规则在 receive-pack 前执行,
/// 实现方在域内(code-review),git-service 经 pre-receive 钩子调用。
#[async_trait::async_trait]
pub trait PushGuard: Send + Sync {
    /// 校验一批推送;拒绝时返回原因(展示给推送者)。
    ///
    /// 返回需要调用方(git 钩子进程)本地执行 force push 判定的引用列表:
    /// pre-receive 期间新对象在隔离区,服务端进程不可见,祖先判定必须
    /// 在钩子内完成(策略在平台,力学在钩子)。
    ///
    /// # Errors
    /// 守卫自身故障(存储不可用)。
    async fn check_push(&self, full_name: &str, refs: &[PushRef]) -> Result<Vec<String>, String>;
}

/// 全零 SHA(新建/删除标记)。
#[must_use]
pub fn is_zero_sha(sha: &str) -> bool {
    sha.bytes().all(|b| b == b'0') || sha.is_empty()
}
