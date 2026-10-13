//! 效能报表指标(T-308 / MS-02):PR 周期、评审响应、交付周期、缺陷逃逸。
//!
//! 口径(v1,写入注释便于复核):
//! - **PR 周期** = PR 创建 → 合并(仅统计已合并的 PR)
//! - **评审响应** = PR 创建 → 该 PR **首次**评审(无评审的 PR 不计入)
//! - **交付周期** = 工作项创建 → 最近更新(仅终态项;v1 以 updated_at 近似完成时刻,
//!   因工作项无独立 completed_at 字段)
//! - **缺陷逃逸率** = 窗口内创建的缺陷中,创建时间**晚于所属迭代结束时间**的占比
//!   (迭代结束后才暴露 = 逃过了该迭代的质量门);无迭代的缺陷不计入分母

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::reports::{ItemRow, is_done};
use crate::stats::{DurationStats, duration_stats};

/// PR 行(只读投影)。
#[derive(Debug, Clone)]
pub struct PrRow {
    /// PR ID。
    pub id: Uuid,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 合并时间(未合并为空)。
    pub merged_at: Option<DateTime<Utc>>,
}

/// 评审行(只读投影)。
#[derive(Debug, Clone)]
pub struct ReviewRow {
    /// 所属 PR。
    pub pr_id: Uuid,
    /// 评审时间。
    pub created_at: DateTime<Utc>,
}

/// PR 周期:创建 → 合并。
#[must_use]
pub fn pr_cycle_time(prs: &[PrRow]) -> DurationStats {
    duration_stats(prs.iter().filter_map(|p| {
        p.merged_at
            .map(|m| (m - p.created_at).num_seconds())
            .filter(|s| *s >= 0)
    }))
}

/// 评审响应:PR 创建 → 首次评审。
#[must_use]
pub fn review_response_time(prs: &[PrRow], reviews: &[ReviewRow]) -> DurationStats {
    // 每个 PR 的首次评审时间
    let mut first: HashMap<Uuid, DateTime<Utc>> = HashMap::new();
    for r in reviews {
        first
            .entry(r.pr_id)
            .and_modify(|cur| {
                if r.created_at < *cur {
                    *cur = r.created_at;
                }
            })
            .or_insert(r.created_at);
    }
    duration_stats(prs.iter().filter_map(|p| {
        first
            .get(&p.id)
            .map(|f| (*f - p.created_at).num_seconds())
            .filter(|s| *s >= 0)
    }))
}

/// 交付周期:终态工作项的创建 → 最近更新。
#[must_use]
pub fn delivery_cycle(items: &[ItemRow]) -> DurationStats {
    duration_stats(
        items
            .iter()
            .filter(|i| is_done(&i.status))
            .map(|i| (i.updated_at - i.created_at).num_seconds())
            .filter(|s| *s >= 0),
    )
}

/// 缺陷逃逸结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DefectEscape {
    /// 计入分母的缺陷数(有迭代归属)。
    pub total: i64,
    /// 逃逸缺陷数(迭代结束后才创建)。
    pub escaped: i64,
    /// 逃逸率百分比。
    pub rate_percent: f64,
}

/// 缺陷逃逸率:迭代结束后才创建的缺陷占比。
#[must_use]
pub fn defect_escape(
    items: &[ItemRow],
    sprint_ends: &HashMap<Uuid, DateTime<Utc>>,
) -> DefectEscape {
    let mut total = 0i64;
    let mut escaped = 0i64;
    for i in items {
        if i.item_type != "bug" {
            continue;
        }
        let Some(sprint_id) = i.sprint_id else {
            continue; // 无迭代归属无法判定,不计入分母
        };
        let Some(end) = sprint_ends.get(&sprint_id) else {
            continue;
        };
        total += 1;
        if i.created_at > *end {
            escaped += 1;
        }
    }
    let rate = if total == 0 {
        0.0
    } else {
        escaped as f64 / total as f64 * 100.0
    };
    DefectEscape {
        total,
        escaped,
        rate_percent: (rate * 100.0).round() / 100.0,
    }
}

/// 效能报表汇总。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EfficiencySummary {
    /// PR 周期。
    pub pr_cycle: DurationStats,
    /// 评审响应。
    pub review_response: DurationStats,
    /// 交付周期。
    pub delivery_cycle: DurationStats,
    /// 缺陷逃逸。
    pub defect_escape: DefectEscape,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn item(kind: &str, status: &str, created: i64, updated: i64, sprint: Option<Uuid>) -> ItemRow {
        ItemRow {
            item_type: kind.to_owned(),
            status: status.to_owned(),
            assignee: None,
            story_points: None,
            sprint_id: sprint,
            created_at: t(created),
            updated_at: t(updated),
        }
    }

    #[test]
    fn pr_cycle_counts_only_merged() {
        let prs = vec![
            PrRow {
                id: Uuid::nil(),
                created_at: t(0),
                merged_at: Some(t(3600)),
            },
            PrRow {
                id: Uuid::from_u128(1),
                created_at: t(0),
                merged_at: None,
            },
        ];
        let s = pr_cycle_time(&prs);
        assert_eq!(s.count, 1, "未合并的 PR 不计入");
        assert_eq!(s.median_seconds, Some(3600));
    }

    #[test]
    fn review_response_uses_first_review_per_pr() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let prs = vec![
            PrRow {
                id: a,
                created_at: t(0),
                merged_at: None,
            },
            PrRow {
                id: b,
                created_at: t(0),
                merged_at: None,
            },
        ];
        // a 有两次评审(取最早 100),b 无评审
        let reviews = vec![
            ReviewRow {
                pr_id: a,
                created_at: t(500),
            },
            ReviewRow {
                pr_id: a,
                created_at: t(100),
            },
        ];
        let s = review_response_time(&prs, &reviews);
        assert_eq!(s.count, 1, "无评审的 PR 不计入");
        assert_eq!(s.median_seconds, Some(100), "取首次评审");
    }

    #[test]
    fn delivery_cycle_only_done_items() {
        let items = vec![
            item("story", "done", 0, 7200, None),
            item("task", "in_progress", 0, 99999, None),
        ];
        let s = delivery_cycle(&items);
        assert_eq!(s.count, 1);
        assert_eq!(s.median_seconds, Some(7200));
    }

    #[test]
    fn defect_escape_counts_bugs_after_sprint_end() {
        let sprint = Uuid::from_u128(9);
        let mut ends = HashMap::new();
        ends.insert(sprint, t(1000));
        let items = vec![
            // 迭代内发现(未逃逸)
            item("bug", "closed", 500, 600, Some(sprint)),
            // 迭代结束后发现(逃逸)
            item("bug", "open", 1500, 1600, Some(sprint)),
            // 非缺陷不计入
            item("story", "open", 1500, 1600, Some(sprint)),
            // 无迭代不计入
            item("bug", "open", 1500, 1600, None),
        ];
        let d = defect_escape(&items, &ends);
        assert_eq!(d.total, 2);
        assert_eq!(d.escaped, 1);
        assert_eq!(d.rate_percent, 50.0);
    }

    #[test]
    fn defect_escape_zero_when_no_sample() {
        let d = defect_escape(&[], &HashMap::new());
        assert_eq!(d.total, 0);
        assert_eq!(d.rate_percent, 0.0);
    }
}
