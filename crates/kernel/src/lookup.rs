//! 用户/组织信息查询抽象:域需要用户展示信息时经此查询,禁止 import identity 内部。

use crate::ids::UserId;

/// 用户简要信息(跨域展示用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserBrief {
    /// 用户 ID。
    pub id: UserId,
    /// 显示名。
    pub display_name: String,
    /// 邮箱。
    pub email: String,
}

/// 身份信息查询抽象(实现见 identity 域)。
#[async_trait::async_trait]
pub trait IdentityLookup: Send + Sync {
    /// 按 ID 查用户简要信息;不存在返回 `Ok(None)`。
    ///
    /// # Errors
    /// 查询自身故障时返回错误。
    async fn user_brief(&self, id: UserId) -> Result<Option<UserBrief>, String>;
}
