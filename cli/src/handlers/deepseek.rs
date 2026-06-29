//! `deepseek` (alias `l2`) — chat.deepseek.com operations.

use crate::types::{CmdResult, kimi, split_arg};
use deepseek::{ChatMode, DeepSeekSemantics, Feature};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let sem = DeepSeekSemantics::from(kimi(&session));

    Ok(match sub {
        "state" => format!("{:?}", sem.get_fast_state().await),
        "ensure" => {
            sem.ensure_tab().await?;
            let s = sem.get_fast_state().await;
            format!("url={} ta={} msgs={}", s.url, s.has_input, s.message_count)
        }
        "send" => {
            if sub_arg.is_empty() { return Err("send requires text".into()); }
            sem.ensure_tab().await?;
            sem.send_message(sub_arg).await?;
            "dispatched".into()
        }
        // ask = send + wait-for-stable + extract in one shot.
        // Use this when you want the full response atomically;
        // use `send` for fire-and-forget followed by manual `extract`.
        "ask" => {
            if sub_arg.is_empty() { return Err("ask requires text".into()); }
            sem.send_and_wait(sub_arg).await?
        }
        "extract" => {
            let r = sem.extract_last_response().await;
            if r.is_empty() { "(empty)".into() } else { r }
        }
        "thinking" => sem.extract_thinking().await.unwrap_or_else(|| "(none)".into()),
        "toggle" => {
            let f = match sub_arg {
                "thinking" | "think" => Feature::Thinking,
                "search" => Feature::Search,
                _ => return Err("toggle requires thinking|search".into()),
            };
            format!("toggle_feature → {}", sem.toggle_feature(f).await)
        }
        "mode" => {
            let m = match sub_arg {
                "quick" => ChatMode::Quick,
                "expert" => ChatMode::Expert,
                _ => return Err("mode requires quick|expert".into()),
            };
            format!("select_mode → {}", sem.select_mode(m).await)
        }
        "new" => { sem.new_conversation().await?; "ok".into() }
        "error" => sem.check_service_error().await.unwrap_or_else(|| "(none)".into()),
        "log" => {
            let entries = sem.get_browser_log(false).await;
            let lines: Vec<String> = entries.iter().map(|e|
                format!("[{}] {}", e.lvl, &e.m[..e.m.len().min(120)])
            ).collect();
            if lines.is_empty() { "(no log entries)".into() } else { lines.join("\n") }
        }
        "button" => format!("send_button_enabled → {}", sem.send_button_enabled().await),
        "scroll" => { sem.scroll_virtual_list().await; "scrolled".into() }
        "turns" => {
            let turns = sem.extract_turns().await;
            if turns.is_empty() { "(no turns found)".into() } else {
                let mut out = String::new();
                for (i, t) in turns.iter().enumerate() {
                    out.push_str(&format!("--- Turn {} ---\n[user] {}\n[model] {}\n\n",
                        i + 1,
                        t.user_message,
                        t.assistant_response,
                    ));
                }
                out
            }
        }
        "open" => {
            if sub_arg.is_empty() { return Err("open requires a session URL or title".into()); }
            sem.open_session(sub_arg).await?;
            "opened".into()
        }
        _ => return Err("subcommands: state ensure send ask extract thinking toggle mode new error log button scroll turns open".into()),
    })
}
