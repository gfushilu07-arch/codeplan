//! 全平台统一的基础强类型 ID(跨域事件 payload 与契约引用)。
//!
//! 域内专属实体 ID(如 BoardId)由各域自行用 `codeplan_common::define_id!` 定义。

use codeplan_common::define_id;

define_id!(
    /// 用户 ID。
    UserId
);
define_id!(
    /// 组织 ID。
    OrgId
);
define_id!(
    /// 团队 ID。
    TeamId
);
define_id!(
    /// 项目 ID。
    ProjectId
);
define_id!(
    /// 工作项 ID。
    WorkItemId
);
define_id!(
    /// 迭代 ID。
    SprintId
);
define_id!(
    /// Wiki 页面 ID。
    WikiPageId
);
define_id!(
    /// 代码仓库 ID。
    RepoId
);
define_id!(
    /// Pull Request ID。
    PullRequestId
);
define_id!(
    /// 流水线 ID。
    PipelineId
);
define_id!(
    /// 应用 ID。
    AppId
);
define_id!(
    /// 发布单 ID。
    ReleaseId
);
