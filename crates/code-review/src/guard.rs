//! 推送门禁实现(T-205):kernel `PushGuard` 契约,分支保护规则落地。
//!
//! 直推/删除在平台直接拒绝;force push 判定因 pre-receive 对象隔离
//! 交给钩子进程(返回需检查的引用列表)。

use std::sync::Arc;

use async_trait::async_trait;
use codeplan_kernel::git::{GitRepoLookup, PushGuard, PushRef, is_zero_sha};
use codeplan_kernel::ids::RepoId;

use crate::protection::ProtectionRepository;

/// 规则守卫:直接推送/删除按 `rev_branch_protection` 规则拦截。
pub struct RulePushGuard {
    protections: Arc<dyn ProtectionRepository>,
    lookup: Arc<dyn GitRepoLookup>,
}

impl RulePushGuard {
    /// 构造。
    #[must_use]
    pub fn new(protections: Arc<dyn ProtectionRepository>, lookup: Arc<dyn GitRepoLookup>) -> Self {
        Self {
            protections,
            lookup,
        }
    }
}

#[async_trait]
impl PushGuard for RulePushGuard {
    async fn check_push(&self, full_name: &str, refs: &[PushRef]) -> Result<Vec<String>, String> {
        let repo = self
            .lookup
            .by_full_name(full_name)
            .await
            .map_err(|e| format!("仓库查询失败: {e}"))?
            .ok_or_else(|| "仓库不存在".to_owned())?;
        let repo_id = RepoId::from_uuid(repo.id.as_uuid());
        let mut rules_matched: Vec<(String, crate::protection::BranchProtection)> = Vec::new();
        for r in refs {
            let Some(branch) = r.ref_name.strip_prefix("refs/heads/") else {
                continue; // v1 只保护分支
            };
            let Some(rule) = self
                .protections
                .find_matching(repo_id, branch)
                .await
                .map_err(|e| e.to_string())?
            else {
                continue;
            };
            rules_matched.push((r.ref_name.clone(), rule.clone()));
            if rule.block_direct_push {
                return Err(format!(
                    "保护分支 {branch}:禁止直接推送,请通过 Pull Request 合并"
                ));
            }
            if is_zero_sha(&r.new) {
                return Err(format!("保护分支 {branch}:禁止删除分支"));
            }
        }
        // 需要钩子本地做祖先判定的引用(保护 + 禁 force + 非新建)
        let force_checks = refs
            .iter()
            .filter(|r| {
                r.ref_name.starts_with("refs/heads/")
                    && !is_zero_sha(&r.old)
                    && rules_matched
                        .iter()
                        .any(|(name, rule)| name == &r.ref_name && rule.block_force_push)
            })
            .map(|r| r.ref_name.clone())
            .collect();
        Ok(force_checks)
    }
}
