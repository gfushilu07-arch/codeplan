//! 状态机引擎(内置于 `crate::workflow`):每类型独立工作流,可被项目级配置覆盖。
//!
//! v1 内置流(所有类型通用基线):
//! `todo → in_progress → in_review → done`;bug 额外 `done → open`(回归)。
//! 自定义配置(JSON)见 [`WorkflowConfig`]。

use serde::{Deserialize, Serialize};

use crate::domain::ItemType;

/// 项目级工作流配置(存 `proj_workflow.config`,缺省用内置)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// 全部合法状态。
    pub states: Vec<String>,
    /// 初始状态。
    pub initial: String,
    /// 合法流转 (from, to)。
    pub transitions: Vec<(String, String)>,
}

/// 状态机:校验"从某状态到某状态"的流转是否合法。
#[derive(Debug, Clone)]
pub struct StateMachine {
    config: WorkflowConfig,
}

impl StateMachine {
    /// 按类型取内置工作流。
    #[must_use]
    pub fn builtin(item_type: ItemType) -> Self {
        let config = match item_type {
            ItemType::Bug => WorkflowConfig {
                states: vec![
                    "open".into(),
                    "in_progress".into(),
                    "resolved".into(),
                    "closed".into(),
                ],
                initial: "open".into(),
                transitions: vec![
                    ("open".into(), "in_progress".into()),
                    ("in_progress".into(), "resolved".into()),
                    ("resolved".into(), "closed".into()),
                    // 回归:验证不通过退回
                    ("resolved".into(), "open".into()),
                    ("closed".into(), "open".into()),
                ],
            },
            _ => WorkflowConfig {
                states: vec![
                    "todo".into(),
                    "in_progress".into(),
                    "in_review".into(),
                    "done".into(),
                ],
                initial: "todo".into(),
                transitions: vec![
                    ("todo".into(), "in_progress".into()),
                    ("in_progress".into(), "in_review".into()),
                    ("in_review".into(), "done".into()),
                    // 打回
                    ("in_review".into(), "in_progress".into()),
                    ("done".into(), "in_progress".into()),
                ],
            },
        };
        Self { config }
    }

    /// 用项目级自定义配置构造(覆盖内置)。
    #[must_use]
    pub fn from_config(config: WorkflowConfig) -> Self {
        Self { config }
    }

    /// 类型的初始状态。
    #[must_use]
    pub fn initial(&self) -> &str {
        &self.config.initial
    }

    /// 判断流转是否合法。
    #[must_use]
    pub fn can_transition(&self, from: &str, to: &str) -> bool {
        self.config
            .transitions
            .iter()
            .any(|(f, t)| f == from && t == to)
    }

    /// 全部合法状态。
    #[must_use]
    pub fn states(&self) -> &[String] {
        &self.config.states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_baseline_flow() {
        let sm = StateMachine::builtin(ItemType::Story);
        assert_eq!(sm.initial(), "todo");
        assert!(sm.can_transition("todo", "in_progress"));
        assert!(sm.can_transition("in_progress", "in_review"));
        assert!(sm.can_transition("in_review", "done"));
        assert!(!sm.can_transition("todo", "done"), "不允许跳状态");
    }

    #[test]
    fn bug_regression_flow() {
        let sm = StateMachine::builtin(ItemType::Bug);
        assert_eq!(sm.initial(), "open");
        assert!(sm.can_transition("resolved", "open"), "缺陷支持回归");
        assert!(!sm.can_transition("open", "resolved"));
    }

    #[test]
    fn custom_config_overrides() {
        let sm = StateMachine::from_config(WorkflowConfig {
            states: vec!["backlog".into(), "shipping".into()],
            initial: "backlog".into(),
            transitions: vec![("backlog".into(), "shipping".into())],
        });
        assert!(sm.can_transition("backlog", "shipping"));
        assert_eq!(sm.states().len(), 2);
    }
}
