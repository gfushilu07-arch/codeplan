//! 分支保护规则实体与仓储 trait(T-205)。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::RepoId;

define_id!(
    /// 分支保护规则 ID。
    ProtectionId
);

/// 分支保护规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchProtection {
    /// 规则 ID。
    pub id: ProtectionId,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 分支模式:精确名或 `*` 结尾前缀通配(如 `release/*`)。
    pub branch_pattern: String,
    /// 合并最少通过人数(0 = 不启用评审门禁)。
    pub min_approvals: i32,
    /// 必须成功的状态检查上下文(空 = 不启用状态门禁)。
    pub required_contexts: Vec<String>,
    /// 禁止 force push(含删除)。
    pub block_force_push: bool,
    /// 禁止直接推送(必须走 PR 合并)。
    pub block_direct_push: bool,
    /// 变更文件的所有者必须至少一人通过(T-206)。
    pub require_code_owner: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 规则写入入参(upsert 语义)。
#[derive(Debug, Clone)]
pub struct NewProtection {
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 分支模式。
    pub branch_pattern: String,
    /// 最少通过人数。
    pub min_approvals: i32,
    /// 必需检查上下文。
    pub required_contexts: Vec<String>,
    /// 禁 force push。
    pub block_force_push: bool,
    /// 禁直推。
    pub block_direct_push: bool,
    /// 必须所有者评审。
    pub require_code_owner: bool,
}

impl BranchProtection {
    /// 分支名是否命中模式(精确,或 `*` 结尾的前缀匹配)。
    #[must_use]
    pub fn matches(&self, branch: &str) -> bool {
        branch_pattern_matches(&self.branch_pattern, branch)
    }
}

/// 模式匹配:精确相等,或模式以 `*` 结尾时做前缀匹配。
#[must_use]
pub fn branch_pattern_matches(pattern: &str, branch: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        branch.starts_with(prefix)
    } else {
        pattern == branch
    }
}

/// 规则仓储。
#[async_trait::async_trait]
pub trait ProtectionRepository: Send + Sync {
    /// 写入或更新规则(仓库 + 模式唯一)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert(&self, rule: &NewProtection) -> Result<BranchProtection, AppError>;

    /// 仓库全部规则。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<BranchProtection>, AppError>;

    /// 命中分支的第一条规则(模式优先精确匹配)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_matching(
        &self,
        repo_id: RepoId,
        branch: &str,
    ) -> Result<Option<BranchProtection>, AppError> {
        let rules = self.list_by_repo(repo_id).await?;
        // 精确匹配优先,再按模式字典序(稳定可预期)
        let mut hits: Vec<&BranchProtection> = rules.iter().filter(|r| r.matches(branch)).collect();
        hits.sort_by(|a, b| {
            let a_exact = (!a.branch_pattern.ends_with('*')) as i8;
            let b_exact = (!b.branch_pattern.ends_with('*')) as i8;
            b_exact
                .cmp(&a_exact)
                .then_with(|| a.branch_pattern.cmp(&b.branch_pattern))
        });
        Ok(hits.first().copied().cloned())
    }

    /// 删除规则(不存在 `CR-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, repo_id: RepoId, branch_pattern: &str) -> Result<(), AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_matching() {
        assert!(branch_pattern_matches("main", "main"));
        assert!(!branch_pattern_matches("main", "master"));
        assert!(branch_pattern_matches("release/*", "release/1.0"));
        assert!(branch_pattern_matches("release/*", "release/"));
        assert!(!branch_pattern_matches("release/*", "release"));
        assert!(!branch_pattern_matches("release/*", "hotfix/1.0"));
    }
}
