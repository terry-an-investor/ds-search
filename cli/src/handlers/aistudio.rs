//! `aistudio` — aistudio.google.com prompt operations.

use crate::types::{CmdResult, kimi, split_arg};
use google_aistudio::{AistudioModel, AistudioSemantics};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let ai = AistudioSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { ai.ensure_tab().await?; "ok".into() }

        "send" => {
            if sub_arg.is_empty() { return Err("send requires a prompt text".into()); }
            ai.send_prompt(sub_arg).await?;
            format!("dispatched: {}", sub_arg)
        }

        "extract" => {
            let r = ai.extract_response().await;
            if r.is_empty() { "(no response yet, use 'aistudio wait' first)".into() }
            else { r }
        }

        "wait" => {
            let ready = ai.wait_for_response(60).await?;
            if ready {
                let r = ai.extract_response().await;
                if r.is_empty() { "response ready but empty".into() }
                else { r }
            } else {
                "timeout waiting for response".into()
            }
        }

        "model" => {
            if sub_arg.is_empty() {
                return Ok(format!("current: {}", ai.current_model().await));
            }
            let m = AistudioModel::from_label(sub_arg)
                .ok_or_else(|| format!("unknown model: {}. Use: flash-lite|flash|pro|flash-image|pro-image|pro-latest|flash-latest|flash-lite-latest", sub_arg))?;
            ai.select_model(m.clone()).await?;
            format!("model → {} ({})", m.display_name(), m.model_id())
        }

        "thinking" => {
            if sub_arg.is_empty() { return Err("thinking requires: low|medium|high".into()); }
            let valid = ["low", "medium", "high"];
            if !valid.contains(&sub_arg.to_lowercase().as_str()) {
                return Err("thinking level must be: low|medium|high".into());
            }
            ai.set_thinking_level(&sub_arg.to_lowercase()).await?;
            format!("thinking level → {}", sub_arg.to_lowercase())
        }

        "new" => { ai.new_chat().await?; "new chat".into() }

        "title" => {
            let t = ai.current_title().await;
            if t.is_empty() { "(no title)".into() } else { t }
        }

        "code" => {
            let c = ai.get_code().await?;
            if c.is_empty() { "(no code)".into() } else { c }
        }

        "history" => {
            ai.go_history().await?;
            let n: usize = sub_arg.parse().unwrap_or(10);
            let items = ai.extract_history(n).await;
            if items.is_empty() { return Ok("(no history items)".into()); }
            let mut out = String::new();
            for (i, it) in items.iter().enumerate() {
                out.push_str(&format!("{}. {} | {}\n", i + 1, it.name, it.time));
            }
            out
        }

        "open" => {
            if sub_arg.is_empty() { return Err("open requires a prompt name from history".into()); }
            ai.open_history_prompt(sub_arg).await?;
            format!("opened: {}", sub_arg)
        }

        _ => return Err("subcommands: ensure send extract wait model thinking new title code history open".into()),
    })
}
