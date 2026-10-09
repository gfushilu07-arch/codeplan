//! 仓库工程任务:`cargo xtask <subcommand>`。
//!
//! - `lint-deps`:crate 依赖白名单检查(解耦规则的机器守门)
//! - `sync-rules`:AGENTS.md → 各 AI 工具规则文件

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "lint-deps" => println!("lint-deps: placeholder"),
        "sync-rules" => println!("sync-rules: placeholder"),
        _ => eprintln!("usage: cargo xtask <lint-deps|sync-rules>"),
    }
}
