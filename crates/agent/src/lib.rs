//! 部署目标主机上的轻量 agent(独立二进制,T-311 / PAAS-03)。
//!
//! 方向:agent **主动外连**平台(主机侧零入站端口),平台侧为 gRPC server。
//!
//! 层级:L2(依赖规则见 ARCHITECTURE.md §1.1,由 xtask lint-deps 强制)。

pub mod deploy;
pub mod executor;

/// 生成的 gRPC 契约(与平台侧共用 `proto/codeplan/agent/v1/agent.proto`)。
pub mod proto {
    #![allow(clippy::doc_markdown, missing_docs)]
    tonic::include_proto!("codeplan.agent.v1");
}

/// agent 版本(随 crate 版本)。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 解析标签串(`a,b , c` → `["a","b","c"]`;忽略空项)。
#[must_use]
pub fn parse_labels(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 心跳间隔:平台建议值优先,但限制在 5~300 秒,避免配置错误导致空转或假离线。
#[must_use]
pub fn effective_interval_secs(suggested: i64) -> u64 {
    if suggested <= 0 {
        return 30;
    }
    (suggested as u64).clamp(5, 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_trimmed_and_filtered() {
        assert_eq!(parse_labels("linux, amd64 ,, gpu "), vec!["linux", "amd64", "gpu"]);
        assert!(parse_labels("   ").is_empty());
    }

    #[test]
    fn interval_is_clamped() {
        assert_eq!(effective_interval_secs(0), 30, "未提供用默认值");
        assert_eq!(effective_interval_secs(-5), 30);
        assert_eq!(effective_interval_secs(1), 5, "下限 5 秒");
        assert_eq!(effective_interval_secs(10_000), 300, "上限 300 秒");
        assert_eq!(effective_interval_secs(30), 30);
    }
}
