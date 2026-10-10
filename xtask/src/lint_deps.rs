//! `lint-deps`:crate 依赖白名单检查(解耦规则的机器守门)。
//!
//! 白名单与 [ARCHITECTURE.md](../../ARCHITECTURE.md) §1.1 的四层规则同步:
//! L1 ← L2 ← L3 ← L4,禁止反向;L3 域之间禁止互相依赖。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// L1 公共层:不依赖任何内部 crate。
const L1: &[&str] = &[];
/// L1 契约层:只准依赖 common。
const L1_KERNEL: &[&str] = &["common"];
/// L2 基础设施:只准依赖 L1(例外:notify 经 ext 的 PluginRegistry 分发通知)。
const L2: &[&str] = &["common", "kernel"];
/// L2 通知:额外依赖 ext(渠道分发契约)。
const L2_NOTIFY: &[&str] = &["common", "kernel", "ext"];
/// L3 业务域:L1 + L2,禁止依赖彼此。
const L3: &[&str] = &["common", "kernel", "eventbus", "storage", "ext", "notify"];
/// L4 组合根:依赖一切。
const L4: &[&str] = &[
    "common",
    "kernel",
    "eventbus",
    "storage",
    "ext",
    "notify",
    "agent",
    "project",
    "wiki",
    "git-service",
    "code-review",
    "ci",
    "release",
    "flags",
    "appcenter",
    "insights",
    "search",
    "ai",
    "identity",
    "eventbus",
    "project",
    "wiki",
    // 官方内置插件(随任务追加)
    "plugin-inbox",
];

/// 目录名 → 允许依赖的内部 crate 目录名列表。
fn allowed_deps(crate_dir: &str) -> Option<&'static [&'static str]> {
    // 官方内置插件:ext + kernel + common + notify(通知存储)
    if let Some(name) = crate_dir.strip_prefix("plugins/") {
        let _ = name;
        return Some(&["common", "kernel", "ext", "notify"]);
    }
    match crate_dir {
        "common" => Some(L1),
        "kernel" => Some(L1_KERNEL),
        "eventbus" | "storage" | "ext" | "agent" => Some(L2),
        "notify" => Some(L2_NOTIFY),
        "server" => Some(L4),
        "xtask" => Some(&[]),
        // L3 业务域
        "project" | "wiki" | "git-service" | "code-review" | "ci" | "release" | "flags"
        | "appcenter" | "insights" | "search" | "ai" | "identity" => Some(L3),
        _ => None,
    }
}

/// 从 Cargo.toml 的 toml::Value 中收集内部依赖目录名(剥去 `codeplan-` 前缀)。
fn internal_deps(manifest: &toml::Value) -> Vec<String> {
    let mut found = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(|v| v.as_table()) {
            for key in table.keys() {
                if let Some(dir) = key.strip_prefix("codeplan-") {
                    found.push(dir.to_owned());
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// 执行检查;有违规时返回含全部明细的 Err。
pub fn run(repo_root: &Path) -> Result<(), String> {
    let crates_dir = repo_root.join("crates");
    let mut dirs: Vec<String> = fs::read_dir(&crates_dir)
        .map_err(|e| format!("读取 {}: {e}", crates_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let plugins_dir = repo_root.join("plugins");
    if plugins_dir.exists() {
        for e in fs::read_dir(&plugins_dir)
            .map_err(|e| format!("读取 {}: {e}", plugins_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("Cargo.toml").exists())
            .filter_map(|e| e.file_name().into_string().ok())
        {
            dirs.push(format!("plugins/{e}"));
        }
    }
    dirs.push("xtask".to_owned());
    dirs.sort();

    let mut violations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut checked = 0usize;

    for dir in &dirs {
        let manifest_path = if dir == "xtask" {
            repo_root.join("xtask/Cargo.toml")
        } else if let Some(plugin) = dir.strip_prefix("plugins/") {
            plugins_dir.join(plugin).join("Cargo.toml")
        } else {
            crates_dir.join(dir).join("Cargo.toml")
        };
        let raw = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("读取 {}: {e}", manifest_path.display()))?;
        let manifest: toml::Value =
            toml::from_str(&raw).map_err(|e| format!("解析 {}: {e}", manifest_path.display()))?;

        let allowed = allowed_deps(dir).ok_or_else(|| {
            format!("crate `{dir}` 未登记层级白名单,请更新 xtask/src/lint_deps.rs")
        })?;
        checked += 1;

        let bad: Vec<String> = internal_deps(&manifest)
            .into_iter()
            .filter(|dep| !allowed.contains(&dep.as_str()))
            .collect();
        if !bad.is_empty() {
            violations.insert(dir.clone(), bad);
        }
    }

    if violations.is_empty() {
        println!("lint-deps: {checked} 个 crate 依赖白名单全部通过 ✓");
        Ok(())
    } else {
        let mut msg = String::from("lint-deps:发现依赖违规(解耦规则见 ARCHITECTURE.md §1.1):\n");
        for (dir, bad) in &violations {
            msg.push_str(&format!("  {dir} → 非法依赖 {bad:?}\n"));
        }
        Err(msg)
    }
}
