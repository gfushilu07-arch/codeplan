//! CODEOWNERS 解析与匹配(T-206,纯函数)。
//!
//! 语法子集:每行 `路径模式 @用户 [@用户 ...]`,`#` 注释,空行忽略;
//! 匹配支持 精确路径 / `前缀*` / `*.后缀`;同文件多条命中时**最后一条**生效
//! (与 GitHub 语义一致)。文件查找顺序:`CODEOWNERS` → `docs/` → `.github/`,
//! 后者优先(GitHub 语义)。

/// 候选路径(查找顺序,后者优先)。
pub const CODEOWNERS_PATHS: [&str; 3] = ["CODEOWNERS", "docs/CODEOWNERS", ".github/CODEOWNERS"];

/// 单条规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeOwnerRule {
    /// 路径模式。
    pub pattern: String,
    /// 所有者用户名(`@` 已剥去,按声明顺序)。
    pub owners: Vec<String>,
}

/// 解析 CODEOWNERS 文本;非法行(无 owner)跳过。
#[must_use]
pub fn parse_codeowners(text: &str) -> Vec<CodeOwnerRule> {
    let mut rules = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(pattern) = tokens.next() else {
            continue;
        };
        let owners: Vec<String> = tokens
            .filter(|t| t.starts_with('@'))
            .map(|t| t.trim_start_matches('@').to_owned())
            .collect();
        if owners.is_empty() {
            continue;
        }
        rules.push(CodeOwnerRule {
            pattern: pattern.to_owned(),
            owners,
        });
    }
    rules
}

/// 模式是否命中路径(精确 / `前缀*` / `*.后缀` / `目录/` 前缀)。
#[must_use]
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        return path.starts_with(prefix);
    }
    if let Some(dir) = pattern.strip_suffix('/') {
        return path.starts_with(&format!("{dir}/"));
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return path.ends_with(suffix);
    }
    pattern == path
}

/// 单文件的生效所有者(最后命中规则优先)。
#[must_use]
pub fn owners_for_file<'a>(rules: &'a [CodeOwnerRule], path: &str) -> Option<&'a [String]> {
    rules
        .iter()
        .rev()
        .find(|r| pattern_matches(&r.pattern, path))
        .map(|r| r.owners.as_slice())
}

/// 按变更文件集合求所有者覆盖表(保留文件声明顺序,去重排序后输出)。
#[must_use]
pub fn owners_by_files(rules: &[CodeOwnerRule], files: &[String]) -> Vec<(String, Vec<String>)> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for f in files {
        if let Some(owners) = owners_for_file(rules, f) {
            let mut sorted: Vec<String> = owners.to_vec();
            sorted.sort();
            sorted.dedup();
            seen.extend(sorted.iter().cloned());
            out.push((f.clone(), sorted));
        }
    }
    let _ = seen;
    out
}

/// 全部涉及的所有者用户名(去重,保持出现顺序)。
#[must_use]
pub fn all_owners(covered: &[(String, Vec<String>)]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (_, owners) in covered {
        for o in owners {
            if seen.insert(o.clone()) {
                out.push(o.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# 注释与空行被忽略
*.rs @alice @bob
docs/ @carol
README.md @dave
/src/lib.rs @alice
"#;

    #[test]
    fn parse_skips_comments_and_requires_owners() {
        let rules = parse_codeowners(SAMPLE);
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].pattern, "*.rs");
        assert_eq!(rules[0].owners, vec!["alice", "bob"]);
        assert_eq!(rules[3].owners, vec!["alice"]);
    }

    #[test]
    fn last_matching_rule_wins() {
        let rules = parse_codeowners(SAMPLE);
        // /src/lib.rs 同时命中 `*.rs` 与 `/src/lib.rs`,后者生效
        assert_eq!(
            owners_for_file(&rules, "/src/app.rs"),
            Some(vec!["alice".to_owned(), "bob".to_owned()].as_slice())
        );
        assert_eq!(
            owners_for_file(&rules, "/src/lib.rs"),
            Some(vec!["alice".to_owned()].as_slice())
        );
        assert_eq!(
            owners_for_file(&rules, "docs/guide.md").map(|o| o.to_vec()),
            Some(vec!["carol".to_owned()])
        );
        assert_eq!(owners_for_file(&rules, "main.py"), None);
    }

    #[test]
    fn coverage_aggregates() {
        let rules = parse_codeowners(SAMPLE);
        let files = vec!["/src/app.rs".to_owned(), "README.md".to_owned()];
        let covered = owners_by_files(&rules, &files);
        assert_eq!(covered.len(), 2);
        assert_eq!(all_owners(&covered), vec!["alice", "bob", "dave"]);
    }
}

/// 从 unified diff 提取变更文件路径(b/~ 侧)。
#[must_use]
pub fn diff_files_pub(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| l.strip_prefix("diff --git a/"))
        .filter_map(|rest| rest.split_once(" b/").map(|(_, p)| p.to_owned()))
        .collect()
}
