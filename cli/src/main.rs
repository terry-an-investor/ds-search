//! ds — browser-automation CLI for web sites via Kimi WebBridge.
//!
//! The registry maps command names to async handler functions. Each handler
//! lives in its own module under `handlers/`. Add a new site:
//!   1. Write `handlers/<name>.rs` exposing `pub async fn handle(...)`.
//!   2. Register it in `registry()` below (one line).
//!   3. Add the module to `handlers/mod.rs`.

mod handlers;
mod types;

use handlers::*;
use pilot::init_logging;
use std::collections::HashMap;
use types::{CmdResult, Handler};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();
    let mut session = "deepseek".to_string();
    let mut cmd_idx = 1;

    if args.len() > 2 && args[1] == "--session" {
        session = args[2].clone();
        cmd_idx = 3;
    }

    if args.len() <= cmd_idx {
        eprintln!("Usage: ds [--session <name>] <command> [args...]");
        eprintln!("Commands: {}", list_commands());
        std::process::exit(1);
    }

    let cmd = args[cmd_idx].clone();
    let arg = if args.len() > cmd_idx + 1 {
        args[cmd_idx + 1..].join(" ")
    } else {
        String::new()
    };

    match run_command(&cmd, &arg, &session).await {
        Ok(output) => {
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

fn list_commands() -> String {
    registry()
        .keys()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

async fn run_command(cmd: &str, arg: &str, session: &str) -> CmdResult {
    let handlers = registry();
    if let Some(handler) = handlers.get(cmd) {
        handler(session.to_string(), arg.to_string()).await
    } else {
        Err(format!("unknown command: {}. Available: {}", cmd, list_commands()).into())
    }
}

fn registry() -> HashMap<&'static str, Handler> {
    let mut m: HashMap<&'static str, Handler> = HashMap::new();
    m.insert("status", Box::new(|s, a| Box::pin(status::handle(s, a))));
    m.insert(
        "deepseek",
        Box::new(|s, a| Box::pin(deepseek::handle(s, a))),
    );
    m.insert("l2", Box::new(|s, a| Box::pin(deepseek::handle(s, a))));
    m.insert("grok", Box::new(|s, a| Box::pin(grok::handle(s, a))));
    m.insert("gemini", Box::new(|s, a| Box::pin(gemini::handle(s, a))));
    m.insert(
        "bilibili",
        Box::new(|s, a| Box::pin(bilibili::handle(s, a))),
    );
    m.insert(
        "wallstreet",
        Box::new(|s, a| Box::pin(wallstreet::handle(s, a))),
    );
    m.insert(
        "livenews",
        Box::new(|s, a| Box::pin(livenews::handle(s, a))),
    );
    m.insert("weread", Box::new(|s, a| Box::pin(weread::handle(s, a))));
    m.insert("google", Box::new(|s, a| Box::pin(google::handle(s, a))));
    m.insert(
        "aistudio",
        Box::new(|s, a| Box::pin(aistudio::handle(s, a))),
    );
    m.insert("x", Box::new(|s, a| Box::pin(x::handle(s, a))));
    m.insert("meta", Box::new(|s, a| Box::pin(meta::handle(s, a))));
    m.insert("raw", Box::new(|s, a| Box::pin(raw::handle(s, a))));
    m
}
