//! IM 回调动作执行(T-211):action_id → 平台动作。
//!
//! 动作词:`pr:approve:{owner}/{key}:{number}`、`pr:reject:…`、
//! `wi:transition:{item_id}:{to}`。用户身份:平台用户名映射
//! (`CODEPLAN_IM_USER_MAP` = `{"platform_user":"username",…}`,缺省拒动作)。

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::UserId;

use crate::im::ImCallback;

/// 动作执行结果(回显给用户)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResult {
    /// 已执行。
    Done(String),
    /// 拒绝(原因)。
    Denied(String),
    /// 未知动作。
    Unknown,
}

/// 动作执行依赖(组合根注入;避免 L2 依赖 L3)。
pub struct ActionDeps {
    /// PR 合并/评审动作回调。
    pub pr_action: PrActionHandler,
    /// 工作项流转回调。
    pub wi_action: WiActionHandler,
    /// 平台用户名映射。
    pub user_map: HashMap<String, String>,
}

/// PR 动作:(username, repo, number, approve) → 结果文本(异步)。
pub type PrActionHandler = Arc<
    dyn Fn(String, String, i64, bool) -> BoxFuture<'static, Result<String, String>> + Send + Sync,
>;

/// 工作项动作:(username, item_id, to) → 结果文本(异步)。
pub type WiActionHandler =
    Arc<dyn Fn(String, String, String) -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

/// 解析 action_id。
#[must_use]
pub fn parse_action(action_id: &str) -> Option<Action> {
    let parts: Vec<&str> = action_id.split(':').collect();
    match parts.as_slice() {
        ["pr", verb @ ("approve" | "reject"), repo, number] => {
            let number = number.parse().ok()?;
            Some(Action::Pr {
                verb: if *verb == "approve" {
                    PrVerb::Approve
                } else {
                    PrVerb::Reject
                },
                repo: (*repo).to_owned(),
                number,
            })
        }
        ["wi", "transition", item_id, to] => Some(Action::WiTransition {
            item_id: (*item_id).to_owned(),
            to: (*to).to_owned(),
        }),
        _ => None,
    }
}

/// 动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// PR 评审动作。
    Pr {
        /// 通过/驳回。
        verb: PrVerb,
        /// 仓库全名。
        repo: String,
        /// PR 序号。
        number: i64,
    },
    /// 工作项流转。
    WiTransition {
        /// 工作项 ID。
        item_id: String,
        /// 目标状态。
        to: String,
    },
}

/// PR 动作词。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrVerb {
    /// 通过。
    Approve,
    /// 驳回。
    Reject,
}

/// 执行回调;无映射用户 → Denied。
pub async fn execute(deps: &ActionDeps, cb: &ImCallback) -> ActionResult {
    let Some(username) = deps.user_map.get(&cb.platform_user).cloned() else {
        return ActionResult::Denied(format!("IM 用户 {} 未绑定平台账号", cb.platform_user));
    };
    let Some(action) = parse_action(&cb.action_id) else {
        eprintln!("[im-action] unknown action_id: {:?}", cb.action_id);
        return ActionResult::Unknown;
    };
    match action {
        Action::Pr { verb, repo, number } => {
            let fut = (deps.pr_action)(username, repo, number, verb == PrVerb::Approve);
            match fut.await {
                Ok(msg) => ActionResult::Done(msg),
                Err(e) => ActionResult::Denied(e),
            }
        }
        Action::WiTransition { item_id, to } => {
            let fut = (deps.wi_action)(username, item_id, to);
            match fut.await {
                Ok(msg) => ActionResult::Done(msg),
                Err(e) => ActionResult::Denied(e),
            }
        }
    }
}

/// 供组合根构造 CurrentUser(动作执行以该用户身份进行)。
#[must_use]
pub fn actor_for(username: &str, uid: UserId) -> CurrentUser {
    CurrentUser {
        id: uid,
        display_name: username.to_owned(),
        email: format!("{username}@im.codeplan.local"),
        is_platform_admin: false,
        scopes: vec!["*".to_owned()],
    }
}
