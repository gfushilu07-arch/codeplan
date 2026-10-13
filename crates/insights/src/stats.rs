//! 统计小工具(insights 域内共用):分位数与时长汇总。
//!
//! 第二个真实调用者出现后才从 `dora` 上提(DORA 与效能报表都要算中位数/p85)。

use serde::Serialize;

/// 最近秩百分位(sorted 升序,`p` ∈ [0,1])。
#[must_use]
pub fn percentile(sorted: &[i64], p: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).max(1) - 1;
    sorted.get(rank.min(sorted.len() - 1)).copied()
}

/// 时长分布(秒)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DurationStats {
    /// 样本数。
    pub count: usize,
    /// 中位数(秒)。
    pub median_seconds: Option<i64>,
    /// p85(秒)。
    pub p85_seconds: Option<i64>,
}

/// 由未排序时长样本汇总(负值与非有限值剔除)。
#[must_use]
pub fn duration_stats(values: impl IntoIterator<Item = i64>) -> DurationStats {
    let mut v: Vec<i64> = values.into_iter().filter(|x| *x >= 0).collect();
    v.sort_unstable();
    DurationStats {
        count: v.len(),
        median_seconds: percentile(&v, 0.5),
        p85_seconds: percentile(&v, 0.85),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let v = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&v, 0.5), Some(30));
        assert_eq!(percentile(&v, 0.85), Some(50));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn duration_stats_sorts_and_drops_negatives() {
        let s = duration_stats([300, -1, 100, 200]);
        assert_eq!(s.count, 3, "负值剔除");
        assert_eq!(s.median_seconds, Some(200));
        assert_eq!(s.p85_seconds, Some(300));
    }
}
