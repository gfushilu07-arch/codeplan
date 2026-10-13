//! DORA 四指标(T-307):事件聚合事实 + 指标计算 + 下钻。
//!
//! 事实口径(v1,部署域未落地前的**明确定义**,非猜测):
//! - `deploy`  默认分支上**成功**的流水线运行(视作一次部署)
//! - `failure` 默认分支上**失败**的流水线运行(视作一次变更失败)
//! - `change`  合入默认分支的 PR(带 lead_time = 合并时刻 − PR 创建时刻)
//!
//! 待批次四发布域落地后,`deploy` 应改由真实发布事件写入;指标算法无需改动。
//!
//! 指标定义(业界标准):
//! - 部署频率 = 窗口内部署次数 / 周数
//! - 变更前置时间 = change 的 lead_time 中位数与 p85
//! - 变更失败率 = failure / (deploy + failure)
//! - MTTR = 每次 failure 到其后**首次部署**的间隔均值(无后续部署的故障不计入)

use codeplan_common::error::AppError;
use serde::Serialize;

use crate::stats::percentile;

/// 域错误码前缀。
const DOMAIN_CODE: &str = "MS";

/// 事实类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    /// 部署(默认分支成功运行)。
    Deploy,
    /// 变更失败(默认分支失败运行)。
    Failure,
    /// 变更(合入默认分支的 PR)。
    Change,
}

impl FactKind {
    /// 入库值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Failure => "failure",
            Self::Change => "change",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "deploy" => Some(Self::Deploy),
            "failure" => Some(Self::Failure),
            "change" => Some(Self::Change),
            _ => None,
        }
    }
}

/// 一条事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    /// 类型。
    pub kind: FactKind,
    /// 发生时刻(UTC,秒级 epoch)。
    pub at: i64,
    /// 前置时间(秒;仅 change 有值)。
    pub lead_time_seconds: Option<i64>,
    /// 摘要。
    pub subject: String,
    /// 引用(sha / PR 号 / 分支)。
    pub reference: String,
}

/// 部署频率结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeploymentFrequency {
    /// 窗口内部署次数。
    pub deployments: i64,
    /// 窗口天数。
    pub days: i64,
    /// 每周部署次数(窗口不足一周按一周算,避免除零放大)。
    pub per_week: f64,
}

/// 前置时间结果(秒)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LeadTime {
    /// 样本数。
    pub count: usize,
    /// 中位数(秒)。
    pub median_seconds: Option<i64>,
    /// p85(秒)。
    pub p85_seconds: Option<i64>,
}

/// 变更失败率结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeFailureRate {
    /// 部署次数。
    pub deployments: i64,
    /// 失败次数。
    pub failures: i64,
    /// 失败率百分比(0~100)。
    pub rate_percent: f64,
}

/// MTTR 结果(秒)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mttr {
    /// 计入的故障数(有后续部署的)。
    pub incidents: usize,
    /// 未恢复故障数(窗口内无后续部署)。
    pub unresolved: usize,
    /// 平均恢复时长(秒)。
    pub mean_seconds: Option<i64>,
}

/// 部署频率:窗口内部署数 / 周数。
#[must_use]
pub fn deployment_frequency(facts: &[Fact], days: i64) -> DeploymentFrequency {
    let deployments = facts
        .iter()
        .filter(|f| f.kind == FactKind::Deploy)
        .count() as i64;
    let weeks = (days.max(1) as f64) / 7.0;
    let per_week = if weeks <= 0.0 {
        0.0
    } else {
        deployments as f64 / weeks
    };
    DeploymentFrequency {
        deployments,
        days: days.max(1),
        per_week: (per_week * 100.0).round() / 100.0,
    }
}

/// 变更前置时间:change 事实的中位数与 p85。
#[must_use]
pub fn lead_time(facts: &[Fact]) -> LeadTime {
    let mut values: Vec<i64> = facts
        .iter()
        .filter(|f| f.kind == FactKind::Change)
        .filter_map(|f| f.lead_time_seconds)
        .filter(|v| *v >= 0)
        .collect();
    values.sort_unstable();
    LeadTime {
        count: values.len(),
        median_seconds: percentile(&values, 0.5),
        p85_seconds: percentile(&values, 0.85),
    }
}

/// 变更失败率:失败 / (部署 + 失败)。
#[must_use]
pub fn change_failure_rate(facts: &[Fact]) -> ChangeFailureRate {
    let deployments = facts
        .iter()
        .filter(|f| f.kind == FactKind::Deploy)
        .count() as i64;
    let failures = facts
        .iter()
        .filter(|f| f.kind == FactKind::Failure)
        .count() as i64;
    let total = deployments + failures;
    let rate = if total == 0 {
        0.0
    } else {
        failures as f64 / total as f64 * 100.0
    };
    ChangeFailureRate {
        deployments,
        failures,
        rate_percent: (rate * 100.0).round() / 100.0,
    }
}

/// MTTR:每次失败到其后首次部署的间隔均值。
#[must_use]
pub fn mttr(facts: &[Fact]) -> Mttr {
    let mut events: Vec<(i64, FactKind)> = facts.iter().map(|f| (f.at, f.kind)).collect();
    events.sort_unstable();
    let mut durations: Vec<i64> = Vec::new();
    let mut unresolved = 0usize;
    for (i, (at, kind)) in events.iter().enumerate() {
        if *kind != FactKind::Failure {
            continue;
        }
        // 其后首个 deploy(时间升序)
        match events[i + 1..]
            .iter()
            .find(|(_, k)| *k == FactKind::Deploy)
        {
            Some((recovered_at, _)) => durations.push(recovered_at - at),
            None => unresolved += 1,
        }
    }
    let mean = if durations.is_empty() {
        None
    } else {
        Some(durations.iter().sum::<i64>() / durations.len() as i64)
    };
    Mttr {
        incidents: durations.len(),
        unresolved,
        mean_seconds: mean,
    }
}

/// 四指标汇总。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoraSummary {
    /// 统计窗口天数。
    pub days: i64,
    /// 部署频率。
    pub deployment_frequency: DeploymentFrequency,
    /// 变更前置时间。
    pub lead_time: LeadTime,
    /// 变更失败率。
    pub change_failure_rate: ChangeFailureRate,
    /// 平均恢复时间。
    pub mttr: Mttr,
}

/// 汇总四指标。
#[must_use]
pub fn summarize(facts: &[Fact], days: i64) -> DoraSummary {
    DoraSummary {
        days: days.max(1),
        deployment_frequency: deployment_frequency(facts, days),
        lead_time: lead_time(facts),
        change_failure_rate: change_failure_rate(facts),
        mttr: mttr(facts),
    }
}

/// 待写入事实。
#[derive(Debug, Clone)]
pub struct NewFact {
    /// 所属仓库。
    pub repo_id: uuid::Uuid,
    /// 类型。
    pub kind: FactKind,
    /// 发生时刻。
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// 前置时间(秒;仅 change)。
    pub lead_time_seconds: Option<i64>,
    /// 摘要。
    pub subject: String,
    /// 引用。
    pub reference: String,
    /// 附加明细。
    pub detail: serde_json::Value,
}

/// 事实查询行:`(kind, occurred_at, lead_time, subject, ref)`。
type FactRow = (String, chrono::DateTime<chrono::Utc>, Option<i64>, String, String);

/// DORA 事实存储。
pub struct PgDoraStore {
    pool: sqlx::PgPool,
}

impl PgDoraStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// 追加事实(按 (repo,kind,ref,occurred_at) 幂等)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn insert(&self, fact: &NewFact) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO ins_dora_fact \
             (id, repo_id, kind, occurred_at, lead_time_seconds, subject, ref, detail) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (repo_id, kind, ref, occurred_at) DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(fact.repo_id)
        .bind(fact.kind.as_str())
        .bind(fact.occurred_at)
        .bind(fact.lead_time_seconds)
        .bind(&fact.subject)
        .bind(&fact.reference)
        .bind(&fact.detail)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(())
    }

    /// 窗口内事实(时间升序,指标计算用)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn facts_since(
        &self,
        repo_id: uuid::Uuid,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Fact>, AppError> {
        let rows: Vec<FactRow> = sqlx::query_as(
                "SELECT kind, occurred_at, lead_time_seconds, subject, ref FROM ins_dora_fact \
                 WHERE repo_id = $1 AND occurred_at >= $2 ORDER BY occurred_at ASC",
            )
            .bind(repo_id)
            .bind(since)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        Ok(rows
            .into_iter()
            .filter_map(|(kind, at, lead, subject, reference)| {
                Some(Fact {
                    kind: FactKind::parse(&kind)?,
                    at: at.timestamp(),
                    lead_time_seconds: lead,
                    subject,
                    reference,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(kind: FactKind, at: i64, lead: Option<i64>) -> Fact {
        Fact {
            kind,
            at,
            lead_time_seconds: lead,
            subject: String::new(),
            reference: String::new(),
        }
    }

    #[test]
    fn deployment_frequency_counts_only_deploys() {
        let facts = vec![
            f(FactKind::Deploy, 0, None),
            f(FactKind::Deploy, 100, None),
            f(FactKind::Failure, 200, None),
            f(FactKind::Change, 300, Some(60)),
        ];
        let d = deployment_frequency(&facts, 14);
        assert_eq!(d.deployments, 2, "失败与变更不计入部署");
        assert_eq!(d.per_week, 1.0, "2 次 / 2 周");
    }

    #[test]
    fn deployment_frequency_avoids_divide_by_zero() {
        let d = deployment_frequency(&[], 0);
        assert_eq!(d.days, 1, "窗口至少 1 天");
        assert_eq!(d.per_week, 0.0);
    }

    #[test]
    fn lead_time_median_and_p85() {
        // 1h, 2h, 3h, 4h, 10h
        let facts: Vec<Fact> = [3600, 7200, 10800, 14400, 36000]
            .iter()
            .map(|s| f(FactKind::Change, 0, Some(*s)))
            .collect();
        let lt = lead_time(&facts);
        assert_eq!(lt.count, 5);
        assert_eq!(lt.median_seconds, Some(10800), "中位数 = 第 3 个(3h)");
        assert_eq!(lt.p85_seconds, Some(36000), "p85 落在最大值");
    }

    #[test]
    fn lead_time_ignores_non_change_and_negative() {
        let facts = vec![
            f(FactKind::Deploy, 0, Some(999)),
            f(FactKind::Change, 1, Some(120)),
            f(FactKind::Change, 2, Some(-5)),
        ];
        let lt = lead_time(&facts);
        assert_eq!(lt.count, 1, "仅统计 change 且非负");
        assert_eq!(lt.median_seconds, Some(120));
    }

    #[test]
    fn change_failure_rate_is_failures_over_total() {
        let facts = vec![
            f(FactKind::Deploy, 0, None),
            f(FactKind::Deploy, 1, None),
            f(FactKind::Deploy, 2, None),
            f(FactKind::Failure, 3, None),
        ];
        let cfr = change_failure_rate(&facts);
        assert_eq!(cfr.deployments, 3);
        assert_eq!(cfr.failures, 1);
        assert_eq!(cfr.rate_percent, 25.0, "1/(3+1)");
    }

    #[test]
    fn change_failure_rate_zero_when_no_events() {
        let cfr = change_failure_rate(&[]);
        assert_eq!(cfr.rate_percent, 0.0);
    }

    #[test]
    fn mttr_measures_failure_to_next_deploy() {
        // 失败@100 → 部署@400(恢复 300);失败@500 之后无部署
        let facts = vec![
            f(FactKind::Deploy, 0, None),
            f(FactKind::Failure, 100, None),
            f(FactKind::Deploy, 400, None),
            f(FactKind::Failure, 500, None),
        ];
        let m = mttr(&facts);
        assert_eq!(m.incidents, 1);
        assert_eq!(m.unresolved, 1, "末尾故障无后续部署");
        assert_eq!(m.mean_seconds, Some(300));
    }

    #[test]
    fn mttr_uses_first_deploy_after_failure() {
        // 失败@100 后有两次部署,取最近一次 250
        let facts = vec![
            f(FactKind::Failure, 100, None),
            f(FactKind::Deploy, 250, None),
            f(FactKind::Deploy, 900, None),
        ];
        assert_eq!(mttr(&facts).mean_seconds, Some(150));
    }

    #[test]
    fn summarize_wires_all_four() {
        let facts = vec![
            f(FactKind::Deploy, 0, None),
            f(FactKind::Failure, 10, None),
            f(FactKind::Deploy, 20, None),
            f(FactKind::Change, 30, Some(3600)),
        ];
        let s = summarize(&facts, 7);
        assert_eq!(s.days, 7);
        assert_eq!(s.deployment_frequency.deployments, 2);
        assert_eq!(s.change_failure_rate.failures, 1);
        assert_eq!(s.lead_time.count, 1);
        assert_eq!(s.mttr.mean_seconds, Some(10));
    }
}
