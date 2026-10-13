//! 安全扫描编排(T-305):Semgrep / gitleaks / trivy 容器执行 + 结果模型。
//!
//! 设计要点:
//! - 扫描在**临时目录**中进行(经 `git archive` 物化引用),不污染裸仓库与工作树;
//! - 容器内把报告写到挂载点内的 `.codeplan-scan.json`,宿主机读回后解析
//!   (避免与 stdout 混流:执行器合并 stdout/stderr);
//! - 三工具各自的 JSON 结构差异收敛在纯函数解析器里,便于单测。

use std::path::Path;
use std::process::Stdio;

use codeplan_common::error::AppError;
use serde::{Deserialize, Serialize};

use crate::DOMAIN_CODE;
use crate::executor::{DockerExecutor, StepSpec};

/// 报告文件名(容器内 /workspace 下,宿主同名读回)。
const REPORT_FILE: &str = ".codeplan-scan.json";

/// 支持的扫描工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScannerTool {
    /// 密钥泄露检测。
    Gitleaks,
    /// SAST(静态应用安全测试)。
    Semgrep,
    /// 依赖/配置漏洞与 IaC。
    Trivy,
}

impl ScannerTool {
    /// 解析工具名。
    #[must_use]
    pub fn from_str_opt(v: &str) -> Option<Self> {
        match v.to_ascii_lowercase().as_str() {
            "gitleaks" => Some(Self::Gitleaks),
            "semgrep" => Some(Self::Semgrep),
            "trivy" => Some(Self::Trivy),
            _ => None,
        }
    }

    /// 工具名(入库值)。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gitleaks => "gitleaks",
            Self::Semgrep => "semgrep",
            Self::Trivy => "trivy",
        }
    }

    /// 容器镜像。
    #[must_use]
    pub fn image(self) -> &'static str {
        match self {
            Self::Gitleaks => "zricethezav/gitleaks:latest",
            Self::Semgrep => "semgrep/semgrep:latest",
            Self::Trivy => "aquasec/trivy:latest",
        }
    }

    /// 容器内命令(报告落 `/workspace/.codeplan-scan.json`)。
    #[must_use]
    pub fn command(self) -> String {
        match self {
            // 发现密钥时默认退出码非 0,显式归零以便照常读报告
            Self::Gitleaks => format!(
                "gitleaks detect --source /workspace --no-git \
                 --report-format json --report-path /workspace/{REPORT_FILE} --exit-code 0"
            ),
            Self::Semgrep => format!(
                "semgrep scan --json --quiet --output /workspace/{REPORT_FILE} /workspace"
            ),
            Self::Trivy => format!(
                "trivy fs --quiet --format json --output /workspace/{REPORT_FILE} /workspace"
            ),
        }
    }

    /// 解析该工具的报告 JSON → 统一发现列表。
    ///
    /// # Errors
    /// JSON 非法。
    pub fn parse(self, report: &str) -> Result<Vec<Finding>, AppError> {
        match self {
            Self::Gitleaks => parse_gitleaks(report),
            Self::Semgrep => parse_semgrep(report),
            Self::Trivy => parse_trivy(report),
        }
    }
}

/// 归一化后的扫描发现。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// 严重程度(critical/high/medium/low/unknown)。
    pub severity: String,
    /// 规则/检查项 ID。
    pub rule_id: String,
    /// 文件路径(相对仓库根)。
    pub file_path: String,
    /// 行号。
    pub line: Option<i64>,
    /// 描述。
    pub message: String,
}

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::invalid(DOMAIN_CODE, msg)
}

/// 归一化严重程度(各工具取值不一)。
fn norm_severity(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" | "error" => "high",
        "medium" | "warning" | "moderate" => "medium",
        "low" | "info" | "note" => "low",
        _ => "unknown",
    }
    .to_owned()
}

/// 去掉容器内 `/workspace/` 前缀,得到仓库相对路径。
fn strip_workspace(p: &str) -> String {
    p.trim_start_matches("/workspace/")
        .trim_start_matches("./")
        .to_owned()
}

// ---------- gitleaks ----------

#[derive(Deserialize)]
struct GitleaksItem {
    #[serde(default)]
    #[serde(rename = "RuleID")]
    rule_id: String,
    #[serde(default)]
    #[serde(rename = "Description")]
    description: String,
    #[serde(default)]
    #[serde(rename = "File")]
    file: String,
    #[serde(default)]
    #[serde(rename = "StartLine")]
    start_line: Option<i64>,
    #[serde(default)]
    #[serde(rename = "Severity")]
    severity: String,
}

fn parse_gitleaks(report: &str) -> Result<Vec<Finding>, AppError> {
    let text = report.trim();
    if text.is_empty() || text == "null" {
        return Ok(Vec::new());
    }
    let items: Vec<GitleaksItem> =
        serde_json::from_str(text).map_err(|e| invalid(format!("gitleaks 报告解析失败:{e}")))?;
    Ok(items
        .into_iter()
        .map(|i| Finding {
            // gitleaks 默认不带 severity,泄露一律按 high 处理
            severity: if i.severity.is_empty() {
                "high".to_owned()
            } else {
                norm_severity(&i.severity)
            },
            rule_id: i.rule_id,
            file_path: strip_workspace(&i.file),
            line: i.start_line,
            message: i.description,
        })
        .collect())
}

// ---------- semgrep ----------

#[derive(Deserialize)]
struct SemgrepReport {
    #[serde(default)]
    results: Vec<SemgrepResult>,
}

#[derive(Deserialize)]
struct SemgrepResult {
    #[serde(default)]
    check_id: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    start: SemgrepPos,
    #[serde(default)]
    extra: SemgrepExtra,
}

#[derive(Deserialize, Default)]
struct SemgrepPos {
    line: Option<i64>,
}

#[derive(Deserialize, Default)]
struct SemgrepExtra {
    #[serde(default)]
    message: String,
    #[serde(default)]
    severity: String,
}

fn parse_semgrep(report: &str) -> Result<Vec<Finding>, AppError> {
    let text = report.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let r: SemgrepReport =
        serde_json::from_str(text).map_err(|e| invalid(format!("semgrep 报告解析失败:{e}")))?;
    Ok(r.results
        .into_iter()
        .map(|i| Finding {
            severity: norm_severity(&i.extra.severity),
            rule_id: i.check_id,
            file_path: strip_workspace(&i.path),
            line: i.start.line,
            message: i.extra.message,
        })
        .collect())
}

// ---------- trivy ----------

#[derive(Deserialize)]
struct TrivyReport {
    #[serde(default)]
    results: Vec<TrivyResult>,
}

#[derive(Deserialize)]
struct TrivyResult {
    #[serde(default)]
    target: String,
    #[serde(default)]
    vulnerabilities: Vec<TrivyVuln>,
}

#[derive(Deserialize)]
struct TrivyVuln {
    #[serde(default, rename = "VulnerabilityID")]
    id: String,
    #[serde(default, rename = "Title")]
    title: String,
    #[serde(default, rename = "Severity")]
    severity: String,
}

fn parse_trivy(report: &str) -> Result<Vec<Finding>, AppError> {
    let text = report.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let r: TrivyReport =
        serde_json::from_str(text).map_err(|e| invalid(format!("trivy 报告解析失败:{e}")))?;
    let mut out = Vec::new();
    for res in r.results {
        for v in res.vulnerabilities {
            out.push(Finding {
                severity: norm_severity(&v.severity),
                rule_id: v.id,
                file_path: strip_workspace(&res.target),
                line: None,
                message: v.title,
            });
        }
    }
    Ok(out)
}

/// 对某裸仓库的引用执行扫描,返回发现列表。
///
/// 临时目录经 `git archive` 物化,扫描结束即清理(不触碰裸仓库与工作树)。
///
/// # Errors
/// 引用物化失败、Docker 不可达、容器执行失败、报告读取/解析失败。
pub async fn run_scan(
    tool: ScannerTool,
    bare_repo: &Path,
    reference: &str,
) -> Result<Vec<Finding>, AppError> {
    let tmp =
        std::env::temp_dir().join(format!("codeplan-scan-{}", uuid::Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&tmp).await.map_err(|e| {
        AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
    })?;

    let result = scan_in(tool, bare_repo, reference, &tmp).await;

    let _ = tokio::fs::remove_dir_all(&tmp).await;
    result
}

async fn scan_in(
    tool: ScannerTool,
    bare_repo: &Path,
    reference: &str,
    tmp: &Path,
) -> Result<Vec<Finding>, AppError> {
    materialize(bare_repo, reference, tmp).await?;
    let exec = DockerExecutor::connect(tool.image())?;
    let outcome = exec
        .run_step(&StepSpec {
            command: tool.command(),
            workdir: tmp.to_path_buf(),
            env: Vec::new(),
            // 扫描镜像自带 ENTRYPOINT(如 gitleaks),须清空才能用 sh -c
            entrypoint: Some(Vec::new()),
        })
        .await?;
    if !outcome.success() {
        return Err(AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(format!(
            "{} 扫描退出码 {}:{}",
            tool.as_str(),
            outcome.exit_code,
            outcome.output.chars().take(300).collect::<String>()
        ))));
    }
    let report = tokio::fs::read_to_string(tmp.join(REPORT_FILE))
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE)
                .with_details(serde_json::json!(format!("扫描报告缺失:{e}")))
        })?;
    tool.parse(&report)
}

/// 把裸仓库的引用物化到目标目录(`git archive | tar -x`)。
async fn materialize(bare_repo: &Path, reference: &str, dest: &Path) -> Result<(), AppError> {
    let archive = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare_repo.display()))
        .args(["archive", reference])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
    if !archive.status.success() {
        return Err(AppError::invalid(
            DOMAIN_CODE,
            format!(
                "引用物化失败({reference}):{}",
                String::from_utf8_lossy(&archive.stderr).trim()
            ),
        ));
    }
    let mut tar = tokio::process::Command::new("tar")
        .args(["-x", "-C"])
        .arg(dest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
    if let Some(mut stdin) = tar.stdin.take() {
        use tokio::io::AsyncWriteExt as _;
        stdin.write_all(&archive.stdout).await.ok();
        stdin.shutdown().await.ok();
    }
    let out = tar
        .wait_with_output()
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
    if !out.status.success() {
        return Err(AppError::internal(DOMAIN_CODE)
            .with_details(serde_json::json!(String::from_utf8_lossy(&out.stderr).to_string())));
    }
    Ok(())
}

// ---------- 发现持久化 ----------

/// 一条已入库的发现(含工具与归属)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredFinding {
    /// 工具。
    pub tool: String,
    /// 严重程度。
    pub severity: String,
    /// 规则 ID。
    pub rule_id: String,
    /// 文件路径。
    pub file_path: String,
    /// 行号。
    pub line: Option<i64>,
    /// 描述。
    pub message: String,
}

/// 扫描发现存储。
pub struct PgScanStore {
    pool: sqlx::PgPool,
}

impl PgScanStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// 覆盖写入某提交的某工具发现(重扫先清旧,保证幂等)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn replace(
        &self,
        repo_id: uuid::Uuid,
        commit_sha: &str,
        tool: ScannerTool,
        findings: &[Finding],
    ) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        sqlx::query("DELETE FROM ci_scan_finding WHERE repo_id = $1 AND commit_sha = $2 AND tool = $3")
            .bind(repo_id)
            .bind(commit_sha)
            .bind(tool.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        for f in findings {
            sqlx::query(
                "INSERT INTO ci_scan_finding \
                 (id, repo_id, commit_sha, tool, severity, rule_id, file_path, line, message) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(repo_id)
            .bind(commit_sha)
            .bind(tool.as_str())
            .bind(&f.severity)
            .bind(&f.rule_id)
            .bind(&f.file_path)
            .bind(f.line)
            .bind(&f.message)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        }
        tx.commit()
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(())
    }

    /// 某提交的全部发现(严重程度降序)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list(
        &self,
        repo_id: uuid::Uuid,
        commit_sha: &str,
    ) -> Result<Vec<StoredFinding>, AppError> {
        let rows: Vec<(String, String, String, String, Option<i32>, String)> = sqlx::query_as(
            "SELECT tool, severity, rule_id, file_path, line, message FROM ci_scan_finding \
             WHERE repo_id = $1 AND commit_sha = $2 \
             ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1 \
                      WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END, file_path, line",
        )
        .bind(repo_id)
        .bind(commit_sha)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(rows
            .into_iter()
            .map(|(tool, severity, rule_id, file_path, line, message)| StoredFinding {
                tool,
                severity,
                rule_id,
                file_path,
                line: line.map(i64::from),
                message,
            })
            .collect())
    }

    /// 按严重程度计数(门禁判定用)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn counts_by_severity(
        &self,
        repo_id: uuid::Uuid,
        commit_sha: &str,
    ) -> Result<std::collections::BTreeMap<String, i64>, AppError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT severity, COUNT(*) FROM ci_scan_finding \
             WHERE repo_id = $1 AND commit_sha = $2 GROUP BY severity",
        )
        .bind(repo_id)
        .bind(commit_sha)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())))?;
        Ok(rows.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitleaks_report_parsed_and_path_normalized() {
        let report = r#"[
            {"RuleID":"aws-access-token","Description":"AWS Access Token",
             "File":"/workspace/src/config.py","StartLine":12}
        ]"#;
        let found = ScannerTool::Gitleaks.parse(report).expect("应解析成功");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule_id, "aws-access-token");
        assert_eq!(found[0].file_path, "src/config.py", "应去掉 /workspace/ 前缀");
        assert_eq!(found[0].line, Some(12));
        assert_eq!(found[0].severity, "high", "gitleaks 无 severity 时按 high");
    }

    #[test]
    fn gitleaks_empty_and_null_reports_are_clean() {
        assert!(ScannerTool::Gitleaks.parse("").expect("空报告").is_empty());
        assert!(ScannerTool::Gitleaks.parse("null").expect("null 报告").is_empty());
        assert!(ScannerTool::Gitleaks.parse("[]").expect("空数组").is_empty());
    }

    #[test]
    fn semgrep_report_parsed_with_severity_mapping() {
        let report = r#"{"results":[
            {"check_id":"python.lang.security.audit.exec","path":"/workspace/app.py",
             "start":{"line":7},"extra":{"message":"detected exec","severity":"ERROR"}},
            {"check_id":"rules.x","path":"./a/b.js","start":{"line":3},
             "extra":{"message":"warn","severity":"WARNING"}}
        ]}"#;
        let found = ScannerTool::Semgrep.parse(report).expect("应解析成功");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].severity, "high", "ERROR → high");
        assert_eq!(found[0].file_path, "app.py");
        assert_eq!(found[1].severity, "medium", "WARNING → medium");
        assert_eq!(found[1].file_path, "a/b.js", "应去掉 ./ 前缀");
    }

    #[test]
    fn trivy_report_parsed_and_flattens_targets() {
        let report = r#"{"results":[
            {"target":"package-lock.json","vulnerabilities":[
                {"VulnerabilityID":"CVE-2024-1","Title":"proto pollution","Severity":"CRITICAL"},
                {"VulnerabilityID":"CVE-2024-2","Title":"xss","Severity":"LOW"}
            ]},
            {"target":"deploy/nginx.conf","vulnerabilities":[]}
        ]}"#;
        let found = ScannerTool::Trivy.parse(report).expect("应解析成功");
        assert_eq!(found.len(), 2, "空 target 不产出发现");
        assert_eq!(found[0].severity, "critical");
        assert_eq!(found[0].rule_id, "CVE-2024-1");
        assert_eq!(found[0].file_path, "package-lock.json");
        assert_eq!(found[1].severity, "low");
    }

    #[test]
    fn unknown_severity_falls_back() {
        assert_eq!(norm_severity("weird"), "unknown");
        assert_eq!(norm_severity("MODERATE"), "medium");
    }

    #[test]
    fn tool_lookup_is_case_insensitive() {
        assert_eq!(ScannerTool::from_str_opt("Gitleaks"), Some(ScannerTool::Gitleaks));
        assert_eq!(ScannerTool::from_str_opt("TRIVY"), Some(ScannerTool::Trivy));
        assert_eq!(ScannerTool::from_str_opt("nope"), None);
    }
}
