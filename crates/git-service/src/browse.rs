//! 代码浏览读路径(T-122):`GitBrowse` adapter——v1 为 git CLI porcelain 实现
//! (与写路径同源,正确性优先;性能敏感后按接口切 gix,见 AGENTS.md 铁律 5)。

use std::path::Path;
use std::process::Stdio;

use codeplan_common::error::AppError;
use serde::Serialize;
use tokio::process::Command;

use crate::DOMAIN_CODE;

fn git_err(e: impl std::fmt::Display) -> AppError {
    AppError::not_found(DOMAIN_CODE, format!("仓库或引用不可读:{e}"))
}

/// 提交摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitDto {
    /// OID。
    pub id: String,
    /// 摘要(首行)。
    pub summary: String,
    /// 作者名。
    pub author_name: String,
    /// 作者邮箱。
    pub author_email: String,
    /// 时间(Unix 秒)。
    pub time: i64,
    /// 父提交。
    pub parents: Vec<String>,
}

/// 树条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeEntryDto {
    /// 名称。
    pub name: String,
    /// 类型:tree/blob。
    pub kind: String,
    /// OID。
    pub oid: String,
    /// 大小(blob)。
    pub size: Option<i64>,
}

/// blame 行块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlameEntry {
    /// 提交 OID。
    pub commit: String,
    /// 作者。
    pub author: String,
    /// 起始行(1 起)。
    pub start_line: usize,
    /// 行数。
    pub line_count: usize,
}

/// Git 浏览 adapter(CLI 实现)。
pub struct GitBrowse {
    repo_path: std::path::PathBuf,
}

impl GitBrowse {
    /// 打开仓库。
    ///
    /// # Errors
    /// 路径不存在。
    pub fn open(repo_path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = repo_path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(git_err("仓库目录不存在"));
        }
        Ok(Self { repo_path: path })
    }

    async fn git(&self, args: &[&str]) -> Result<String, AppError> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        if !out.status.success() {
            return Err(git_err(String::from_utf8_lossy(&out.stderr)));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// HEAD/引用解析为 OID。
    ///
    /// # Errors
    /// 引用不存在。
    pub async fn resolve(&self, reference: &str) -> Result<String, AppError> {
        let out = self.git(&["rev-parse", reference]).await?;
        out.lines()
            .next()
            .map(str::to_owned)
            .ok_or_else(|| git_err("引用解析为空"))
    }

    /// 单层文件树条目(目录优先)。
    ///
    /// # Errors
    /// 引用/路径不存在。
    pub async fn tree_entries(
        &self,
        reference: &str,
        path: &str,
    ) -> Result<Vec<TreeEntryDto>, AppError> {
        let arg = if path.is_empty() {
            reference.to_owned()
        } else {
            format!("{reference}:{path}")
        };
        let out = self.git(&["ls-tree", "-l", &arg]).await?;
        let mut entries: Vec<TreeEntryDto> = out
            .lines()
            .filter_map(|line| {
                // "<mode> <type> <oid> <size>\t<name>";size 对 tree 为 "-"
                let (meta, name) = line.split_once('\t')?;
                let mut parts = meta.split_whitespace();
                let kind = parts.next()?;
                let oid = parts.next()?;
                let size = parts.next().and_then(|s| s.parse().ok());
                let kind = if kind == "tree" { "tree" } else { "blob" };
                Some(TreeEntryDto {
                    name: name.to_owned(),
                    kind: kind.to_owned(),
                    oid: oid.to_owned(),
                    size,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            (b.kind == "tree")
                .cmp(&(a.kind == "tree"))
                .then(a.name.cmp(&b.name))
        });
        Ok(entries)
    }

    /// 提交历史(首父遍历,新→旧)。
    ///
    /// # Errors
    /// 引用不存在。
    pub async fn commit_log(
        &self,
        reference: &str,
        limit: usize,
    ) -> Result<Vec<CommitDto>, AppError> {
        let limit_str = limit.to_string();
        let format = "%H\x1f%an\x1f%ae\x1f%ct\x1f%P\x1f%s\x1e";
        let out = self
            .git(&[
                "log",
                &format!("--max-count={limit_str}"),
                &format!("--pretty=format:{format}"),
                reference,
            ])
            .await?;
        Ok(out
            .split('\x1e')
            .filter(|record| !record.trim().is_empty())
            .filter_map(|record| {
                let fields: Vec<&str> = record.trim_start_matches('\n').split('\x1f').collect();
                Some(CommitDto {
                    id: fields.first().map(|v| (*v).to_owned())?,
                    author_name: fields.get(1).map(|v| (*v).to_owned())?,
                    author_email: fields.get(2).map(|v| (*v).to_owned())?,
                    time: fields.get(3).and_then(|v| v.parse().ok())?,
                    parents: fields
                        .get(4)
                        .map(|v| v.split_whitespace().map(str::to_owned).collect())
                        .unwrap_or_default(),
                    summary: fields.get(5).map(|v| (*v).to_owned()).unwrap_or_default(),
                })
            })
            .collect())
    }

    /// 单提交详情 + 变更文件(path → A/M/D)。
    ///
    /// # Errors
    /// 提交不存在。
    pub async fn commit_detail(
        &self,
        oid: &str,
    ) -> Result<(CommitDto, Vec<(String, char)>), AppError> {
        let log = self
            .git(&[
                "show",
                "--pretty=format:%H\x1f%an\x1f%ae\x1f%ct\x1f%P\x1f%s",
                "--name-status",
                oid,
            ])
            .await?;
        let mut lines = log.lines();
        let header = lines.next().unwrap_or_default();
        let fields: Vec<&str> = header.split('\x1f').collect();
        let dto = CommitDto {
            id: fields
                .first()
                .map(|v| (*v).to_owned())
                .ok_or_else(|| git_err("提交头缺 OID"))?,
            author_name: fields
                .get(1)
                .map(|v| (*v).to_owned())
                .ok_or_else(|| git_err("提交头缺作者"))?,
            author_email: fields.get(2).map(|v| (*v).to_owned()).unwrap_or_default(),
            time: fields.get(3).and_then(|v| v.parse().ok()).unwrap_or(0),
            parents: fields
                .get(4)
                .map(|v| v.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
            summary: fields.get(5).map(|v| (*v).to_owned()).unwrap_or_default(),
        };
        let changes = lines
            .filter_map(|l| {
                let (status, path) = l.split_once('\t')?;
                Some((path.to_owned(), status.chars().next()?))
            })
            .collect();
        Ok((dto, changes))
    }

    /// 文件内容。
    ///
    /// # Errors
    /// 文件不存在。
    pub async fn blob_content(&self, reference: &str, path: &str) -> Result<Vec<u8>, AppError> {
        let arg = format!("{reference}:{path}");
        let out = Command::new("git")
            .args(["cat-file", "blob", &arg])
            .current_dir(&self.repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        if !out.status.success() {
            return Err(git_err(String::from_utf8_lossy(&out.stderr)));
        }
        Ok(out.stdout)
    }

    /// 两引用间变更文件:path → A/M/D。
    ///
    /// # Errors
    /// 引用不存在。
    pub async fn diff_names(
        &self,
        from_ref: &str,
        to_ref: &str,
    ) -> Result<Vec<(String, char)>, AppError> {
        let out = self
            .git(&["diff", "--name-status", from_ref, to_ref])
            .await?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let (status, path) = l.split_once('\t')?;
                Some((path.to_owned(), status.chars().next()?))
            })
            .collect())
    }

    /// blame(porcelain 解析,按提交聚合)。
    ///
    /// # Errors
    /// 文件不在历史中。
    pub async fn blame(&self, reference: &str, path: &str) -> Result<Vec<BlameEntry>, AppError> {
        let out = self
            .git(&["blame", "--porcelain", reference, "--", path])
            .await?;
        let mut entries: Vec<BlameEntry> = Vec::new();
        let commit_meta: std::collections::HashMap<String, (String, usize)> =
            std::collections::HashMap::new();
        for line in out.lines() {
            if let Some((oid, rest)) = line.split_once(' ')
                && rest.split(' ').count() >= 3
                && !line.starts_with('\t')
            {
                // 头部行:<oid> <origLine> <finalLine> [<numLines>]
                let parts: Vec<&str> = rest.split(' ').collect();
                let start: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                let count: usize = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
                let (author, _) = commit_meta.get(oid).cloned().unwrap_or_default();
                entries.push(BlameEntry {
                    commit: oid.to_owned(),
                    author,
                    start_line: start,
                    line_count: count,
                });
                continue;
            }
            if let Some(author) = line.strip_prefix("author ") {
                // 归属到最近一条无作者的 blame 块
                if let Some(last) = entries.iter_mut().rev().find(|e| e.author.is_empty()) {
                    last.author = author.to_owned();
                }
                let _ = commit_meta;
            }
        }
        Ok(entries)
    }
}
