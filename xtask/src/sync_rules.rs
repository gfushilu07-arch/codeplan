//! `sync-rules`:从 AGENTS.md(单一事实来源)生成各 AI 工具的规则文件。
//!
//! 生成物(均带"勿手改"标记,可随时重跑覆盖):
//! - `CLAUDE.md`:@AGENTS.md 指针
//! - `.cursor/rules/codeplan.mdc`:根规则(alwaysApply)
//! - `.cursor/rules/codeplan-crates.mdc`:crates 规则(globs: crates/**)
//! - `.cursor/rules/codeplan-web.mdc`:web 规则(globs: web/**)
//! - `.github/copilot-instructions.md`:根规则全文

use std::fs;
use std::path::Path;

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取 {}: {e}", path.display())),
    }
}

fn read_required(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("读取 {}: {e}", path.display()))
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 {}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("写入 {}: {e}", path.display()))?;
    println!("  写入 {}", path.display());
    Ok(())
}

fn cursor_frontmatter(description: &str, globs: Option<&str>) -> String {
    match globs {
        Some(g) => {
            format!("---\ndescription: {description}\nglobs: {g}\nalwaysApply: false\n---\n\n")
        }
        None => format!("---\ndescription: {description}\nalwaysApply: true\n---\n\n"),
    }
}

/// 执行同步;返回写入文件数。
pub fn run(repo_root: &Path) -> Result<(), String> {
    println!("sync-rules:从 AGENTS.md 生成工具规则文件");

    let root_rules = read_required(&repo_root.join("AGENTS.md"))?;

    // CLAUDE.md:@import 指针(Claude Code / ZCode 兼容)
    write(
        &repo_root.join("CLAUDE.md"),
        "<!-- 由 xtask sync-rules 生成,勿手改;事实来源:AGENTS.md -->\n\n@AGENTS.md\n",
    )?;

    // Cursor 规则
    let root_doc =
        cursor_frontmatter("CodePlan 协作规则(生成自根 AGENTS.md,勿手改)", None) + &root_rules;
    write(&repo_root.join(".cursor/rules/codeplan.mdc"), &root_doc)?;

    if let Some(rules) = read_optional(&repo_root.join("crates/AGENTS.md"))? {
        let doc = cursor_frontmatter(
            "后端域规则(生成自 crates/AGENTS.md,勿手改)",
            Some("crates/**"),
        ) + &rules;
        write(&repo_root.join(".cursor/rules/codeplan-crates.mdc"), &doc)?;
    }
    if let Some(rules) = read_optional(&repo_root.join("web/AGENTS.md"))? {
        let doc =
            cursor_frontmatter("前端规则(生成自 web/AGENTS.md,勿手改)", Some("web/**")) + &rules;
        write(&repo_root.join(".cursor/rules/codeplan-web.mdc"), &doc)?;
    }

    // GitHub Copilot
    write(
        &repo_root.join(".github/copilot-instructions.md"),
        &format!("<!-- 由 xtask sync-rules 生成,勿手改;事实来源:AGENTS.md -->\n\n{root_rules}"),
    )?;

    println!("sync-rules:完成 ✓");
    Ok(())
}
