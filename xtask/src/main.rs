//! 仓库工程任务:`cargo xtask <subcommand>`。
//!
//! - `lint-deps`:crate 依赖白名单检查(解耦规则的机器守门)
//! - `sync-rules`:AGENTS.md → 各 AI 工具规则文件

mod lint_deps;
mod sync_rules;

use std::path::PathBuf;

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let result = match repo_root() {
        Ok(root) => match cmd.as_str() {
            "lint-deps" => lint_deps::run(&root),
            "sync-rules" => sync_rules::run(&root),
            _ => Err("usage: cargo xtask <lint-deps|sync-rules>".to_owned()),
        },
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// 从当前目录向上找仓库根(须同时含 Cargo.toml 与 AGENTS.md)。
fn repo_root() -> Result<PathBuf, String> {
    let mut dir = std::env::current_dir().map_err(|e| format!("无法获取当前目录:{e}"))?;
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("AGENTS.md").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("未找到仓库根(须含 Cargo.toml 与 AGENTS.md)".to_owned());
        }
    }
}
