//! 代码搜索索引(T-306):把仓库文本文件作为可检索文档纳入 tantivy。
//!
//! 只索引**文本类**文件并设体积/数量上限,避免大仓库与二进制污染索引。
//! 读路径走 git CLI porcelain(`--git-dir` 直读裸仓库),不引入 git2/libgit2。

use std::path::Path;
use std::process::Stdio;

use codeplan_common::error::AppError;

use crate::index::{DocKind, SearchDoc};

/// 单文件索引上限(超出跳过)。
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// 单仓库全量索引文件数上限。
pub const MAX_FILES_PER_REPO: usize = 2000;

/// 可索引的文本扩展名。
const TEXT_EXT: &[&str] = &[
    "rs", "go", "py", "js", "jsx", "ts", "tsx", "java", "kt", "c", "h", "cc", "cpp", "hpp", "cs",
    "rb", "php", "swift", "m", "mm", "scala", "sh", "bash", "zsh", "fish", "sql", "toml", "yaml",
    "yml", "json", "xml", "html", "css", "scss", "less", "md", "markdown", "txt", "ini", "conf",
    "cfg", "env", "gradle", "proto", "tf", "dockerfile", "makefile", "cmake", "lua", "pl", "r",
    "dart", "ex", "exs", "erl", "hs", "clj", "vue", "svelte",
];

/// 跳过的路径片段(依赖目录/构建产物/二进制)。
const SKIP_SEGMENTS: &[&str] = &[
    "/.git/", "/node_modules/", "/target/", "/dist/", "/build/", "/vendor/", "/.venv/",
    "/__pycache__/", "/.next/", "/coverage/",
];

/// 判断某文件是否值得索引(纯函数,便于单测)。
#[must_use]
pub fn is_indexable(path: &str, size: usize) -> bool {
    if path.is_empty() || size == 0 || size > MAX_FILE_BYTES {
        return false;
    }
    let lower = format!("/{}", path.to_ascii_lowercase());
    if SKIP_SEGMENTS.iter().any(|s| lower.contains(s)) {
        return false;
    }
    let name = lower.rsplit('/').next().unwrap_or("");
    // 无扩展名但名字即约定的文件
    if name == "dockerfile" || name == "makefile" || name == "cmakelists.txt" {
        return true;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    TEXT_EXT.contains(&ext)
}

/// 文档 ID:`code:{repo_id}:{path}`。
#[must_use]
pub fn doc_id(repo_id: uuid::Uuid, path: &str) -> String {
    format!("code:{repo_id}:{path}")
}

/// 执行 git(裸仓库)。
async fn git(bare: &Path, args: &[&str]) -> Result<String, AppError> {
    let out = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare.display()))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
    if !out.status.success() {
        return Err(AppError::internal("SR").with_details(serde_json::json!(format!(
            "git {} 失败:{}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 读某提交下某文件内容(缺失/非文本返回 None)。
async fn file_at(bare: &Path, sha: &str, path: &str) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare.display()))
        .arg("show")
        .arg(format!("{sha}:{path}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() || out.stdout.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// 某提交**变更**的文本文件 → 待索引文档(增量路径)。
///
/// # Errors
/// git 调用失败。
pub async fn docs_for_commit(
    bare: &Path,
    repo_id: uuid::Uuid,
    sha: &str,
) -> Result<Vec<SearchDoc>, AppError> {
    if sha.is_empty() {
        return Ok(Vec::new());
    }
    // 根提交无父:--root 让其列出全部文件
    let changed = git(
        bare,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--root",
            sha,
        ],
    )
    .await?;
    let mut docs = Vec::new();
    for path in changed.lines().filter(|l| !l.trim().is_empty()) {
        let Some(body) = file_at(bare, sha, path).await else {
            continue;
        };
        if !is_indexable(path, body.len()) {
            continue;
        }
        docs.push(SearchDoc {
            id: doc_id(repo_id, path),
            kind: DocKind::Code,
            project_id: None,
            title: path.to_owned(),
            body,
        });
    }
    Ok(docs)
}

/// 某引用的**全部**文本文件 → 待索引文档(全量重建路径,带文件数上限)。
///
/// # Errors
/// git 调用失败。
pub async fn docs_for_ref(
    bare: &Path,
    repo_id: uuid::Uuid,
    reference: &str,
) -> Result<Vec<SearchDoc>, AppError> {
    // 注意:`--name-only` 不能与 `-l` 同用(体积由读回内容时判定)
    let listing = git(bare, &["ls-tree", "-r", "--name-only", reference]).await?;
    let mut docs = Vec::new();
    for path in listing.lines().filter(|l| !l.trim().is_empty()) {
        if docs.len() >= MAX_FILES_PER_REPO {
            break;
        }
        let Some(body) = file_at(bare, reference, path).await else {
            continue;
        };
        if !is_indexable(path, body.len()) {
            continue;
        }
        docs.push(SearchDoc {
            id: doc_id(repo_id, path),
            kind: DocKind::Code,
            project_id: None,
            title: path.to_owned(),
            body,
        });
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_common_text_files() {
        assert!(is_indexable("src/main.rs", 120));
        assert!(is_indexable("README.md", 900));
        assert!(is_indexable("deploy/docker-compose.yml", 300));
        assert!(is_indexable("Dockerfile", 200));
        assert!(is_indexable("Makefile", 200));
    }

    #[test]
    fn skips_binaries_and_oversized() {
        assert!(!is_indexable("logo.png", 100), "图片不索引");
        assert!(!is_indexable("app.wasm", 100));
        assert!(!is_indexable("src/main.rs", MAX_FILE_BYTES + 1), "超限跳过");
        assert!(!is_indexable("empty.rs", 0));
        assert!(!is_indexable("", 10));
    }

    #[test]
    fn skips_vendor_and_build_dirs() {
        assert!(!is_indexable("node_modules/react/index.js", 100));
        assert!(!is_indexable("target/debug/build.rs", 100));
        assert!(!is_indexable("web/dist/bundle.js", 100));
        assert!(!is_indexable("vendor/lib/x.go", 100));
    }

    #[test]
    fn doc_id_is_repo_scoped() {
        let a = uuid::Uuid::nil();
        assert_eq!(doc_id(a, "src/main.rs"), format!("code:{a}:src/main.rs"));
    }
}
