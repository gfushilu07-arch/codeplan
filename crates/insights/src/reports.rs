//! 报表计算(T-212):迭代报告 / 负载 / 缺陷趋势 / 吞吐 / 累计流。
//!
//! 数据源 = `proj_*` 只读快照(横切分析域,SQL 直读;写入仍归各域所有)。
//! 计算为纯函数(输入行结构体),便于单测。

use chrono::{DateTime, Datelike, Utc};

/// 工作项只读投影。
#[derive(Debug, Clone)]
pub struct ItemRow {
    /// 类型(task/story/bug/epic)。
    pub item_type: String,
    /// 状态。
    pub status: String,
    /// 经办人(可空)。
    pub assignee: Option<uuid::Uuid>,
    /// 故事点。
    pub story_points: Option<i64>,
    /// 所属迭代。
    pub sprint_id: Option<uuid::Uuid>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 终态判定(两类工作流)。
#[must_use]
pub fn is_done(status: &str) -> bool {
    matches!(status, "done" | "closed")
}

/// 迭代报告。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SprintReport {
    /// 迭代工作项总数。
    pub total: i64,
    /// 完成数。
    pub done: i64,
    /// 未完成数。
    pub open: i64,
    /// 故事点总数。
    pub points_total: i64,
    /// 完成故事点。
    pub points_done: i64,
    /// 完成率(0-100,四舍五入;total=0 时 0)。
    pub done_ratio: i64,
}

/// 计算迭代报告。
#[must_use]
pub fn sprint_report(items: &[ItemRow], sprint_id: uuid::Uuid) -> SprintReport {
    let scoped: Vec<&ItemRow> = items
        .iter()
        .filter(|i| i.sprint_id == Some(sprint_id))
        .collect();
    let total = scoped.len() as i64;
    let done = scoped.iter().filter(|i| is_done(&i.status)).count() as i64;
    let points_total: i64 = scoped.iter().filter_map(|i| i.story_points).sum();
    let points_done: i64 = scoped
        .iter()
        .filter(|i| is_done(&i.status))
        .filter_map(|i| i.story_points)
        .sum();
    let done_ratio = if total == 0 {
        0
    } else {
        (done * 100 + total / 2) / total
    };
    SprintReport {
        total,
        done,
        open: total - done,
        points_total,
        points_done,
        done_ratio,
    }
}

/// 负载条目(按经办人)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AssigneeLoad {
    /// 经办人 UUID。
    pub assignee: uuid::Uuid,
    /// 未完成项数。
    pub open_items: i64,
    /// 未完成故事点。
    pub open_points: i64,
}

/// 计算负载(未完成项按经办人聚合,降序)。
#[must_use]
pub fn assignee_load(items: &[ItemRow]) -> Vec<AssigneeLoad> {
    let mut map: std::collections::HashMap<uuid::Uuid, (i64, i64)> =
        std::collections::HashMap::new();
    for i in items {
        if is_done(&i.status) {
            continue;
        }
        if let Some(a) = i.assignee {
            let e = map.entry(a).or_insert((0, 0));
            e.0 += 1;
            e.1 += i.story_points.unwrap_or(0);
        }
    }
    let mut out: Vec<AssigneeLoad> = map
        .into_iter()
        .map(|(assignee, (open_items, open_points))| AssigneeLoad {
            assignee,
            open_items,
            open_points,
        })
        .collect();
    out.sort_by(|a, b| {
        b.open_items
            .cmp(&a.open_items)
            .then(a.assignee.cmp(&b.assignee))
    });
    out
}

/// 每日缺陷趋势点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BugTrendPoint {
    /// 日期(UTC,YYYY-MM-DD)。
    pub date: String,
    /// 当日新建 bug 数。
    pub created: i64,
    /// 当日完结(进入终态)bug 数。
    pub closed: i64,
}

/// 缺陷趋势(近 days 天,按 UTC 日分桶,升序补零)。
#[must_use]
pub fn bug_trend(items: &[ItemRow], days: u32) -> Vec<BugTrendPoint> {
    daily_trend(items.iter().filter(|i| i.item_type == "bug"), days)
}

fn daily_trend<'a, I>(bugs: I, days: u32) -> Vec<BugTrendPoint>
where
    I: Iterator<Item = &'a ItemRow>,
{
    let today = Utc::now().date_naive();
    let mut map: std::collections::HashMap<chrono::NaiveDate, (i64, i64)> =
        std::collections::HashMap::new();
    for b in bugs {
        let c = b.created_at.date_naive();
        map.entry(c).or_insert((0, 0)).0 += 1;
        if is_done(&b.status) {
            let d = b.updated_at.date_naive();
            map.entry(d).or_insert((0, 0)).1 += 1;
        }
    }
    (0..days)
        .rev()
        .map(|back| {
            let date = today - chrono::Duration::days(i64::from(back));
            let (created, closed) = map.get(&date).copied().unwrap_or((0, 0));
            BugTrendPoint {
                date: date.to_string(),
                created,
                closed,
            }
        })
        .collect()
}

/// 每日累计流点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CfdPoint {
    /// 日期。
    pub date: String,
    /// 累计新建。
    pub created: i64,
    /// 累计完结。
    pub done: i64,
    /// 在制品(累计新建 - 累计完结)。
    pub wip: i64,
}

/// 累计流(近 days 天,基于当前快照的创建/完结时间)。
#[must_use]
pub fn cumulative_flow(items: &[ItemRow], days: u32) -> Vec<CfdPoint> {
    let trend = daily_trend(items.iter(), days);
    let mut created = 0i64;
    let mut done = 0i64;
    trend
        .into_iter()
        .map(|p| {
            created += p.created;
            done += p.closed;
            CfdPoint {
                date: p.date,
                created,
                done,
                wip: created - done,
            }
        })
        .collect()
}

/// 吞吐点(每周完结数)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ThroughputPoint {
    /// 周起始日(周一)。
    pub week: String,
    /// 当周完结数。
    pub done: i64,
}

/// 吞吐(近 weeks 周完结,按 ISO 周一分桶,升序补零)。
#[must_use]
pub fn throughput(items: &[ItemRow], weeks: u32) -> Vec<ThroughputPoint> {
    let today = Utc::now().date_naive();
    let this_monday =
        today - chrono::Duration::days(i64::from(today.weekday().num_days_from_monday()));
    let mut map: std::collections::HashMap<chrono::NaiveDate, i64> =
        std::collections::HashMap::new();
    for i in items {
        if is_done(&i.status) {
            let monday = i.updated_at.date_naive()
                - chrono::Duration::days(i64::from(
                    i.updated_at.date_naive().weekday().num_days_from_monday(),
                ));
            *map.entry(monday).or_insert(0) += 1;
        }
    }
    (0..weeks)
        .rev()
        .map(|back| {
            let week = this_monday - chrono::Duration::weeks(i64::from(back));
            ThroughputPoint {
                week: week.to_string(),
                done: map.get(&week).copied().unwrap_or(0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        status: &str,
        kind: &str,
        points: Option<i64>,
        assignee: Option<uuid::Uuid>,
        sprint: Option<uuid::Uuid>,
    ) -> ItemRow {
        ItemRow {
            item_type: kind.to_owned(),
            status: status.to_owned(),
            assignee,
            story_points: points,
            sprint_id: sprint,
            created_at: Utc::now() - chrono::Duration::hours(2),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn sprint_report_counts() {
        let sprint = uuid::Uuid::new_v4();
        let items = vec![
            item("done", "task", Some(3), None, Some(sprint)),
            item("in_progress", "bug", Some(2), None, Some(sprint)),
            item("done", "story", Some(5), None, Some(sprint)),
            item("done", "story", None, None, None), // 不属于迭代
        ];
        let r = sprint_report(&items, sprint);
        assert_eq!(
            r,
            SprintReport {
                total: 3,
                done: 2,
                open: 1,
                points_total: 10,
                points_done: 8,
                done_ratio: 67,
            }
        );
    }

    #[test]
    fn load_sorted_desc() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let items = vec![
            item("todo", "task", Some(1), Some(a), None),
            item("todo", "task", Some(2), Some(a), None),
            item("todo", "bug", None, Some(b), None),
            item("done", "task", Some(9), Some(b), None),
        ];
        let loads = assignee_load(&items);
        assert_eq!(loads.len(), 2);
        assert_eq!(loads[0].assignee, a);
        assert_eq!(loads[0].open_items, 2);
        assert_eq!(loads[0].open_points, 3);
        assert_eq!(loads[1].open_points, 0);
    }

    #[test]
    fn trend_buckets_and_cfd() {
        let bug_done = item("closed", "bug", None, None, None);
        let mut items = vec![bug_done];
        for _ in 0..2 {
            items.push(item("open", "bug", None, None, None));
        }
        items.push(item("todo", "task", None, None, None)); // 非 bug 不计入
        let trend = bug_trend(&items, 3);
        assert_eq!(trend.len(), 3);
        assert_eq!(trend[2].created, 3, "今日创建 3 个 bug");
        assert_eq!(trend[2].closed, 1);

        // 累计流含全部工作项(3 bug + 1 task)
        let cfd = cumulative_flow(&items, 3);
        assert_eq!(cfd.len(), 3);
        assert_eq!(cfd[2].created, 4);
        assert_eq!(cfd[2].done, 1);
        assert_eq!(cfd[2].wip, 3);
        assert_eq!(cfd[0].created, 0, "首日补零");
    }

    #[test]
    fn throughput_weeks() {
        let items = vec![item("closed", "bug", None, None, None)];
        let t = throughput(&items, 4);
        assert_eq!(t.len(), 4);
        assert_eq!(t[3].done, 1, "本周完结 1");
        assert_eq!(t[0].done, 0, "历史周补零");
    }
}
