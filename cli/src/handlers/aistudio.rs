//! `aistudio` — aistudio.google.com prompt operations.

use crate::types::{CmdResult, kimi, split_arg};
use google_aistudio::{AistudioModel, AistudioSemantics, ThinkingLevel, Tool};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let ai = AistudioSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => {
            ai.ensure_tab().await?;
            "ok".into()
        }

        // Manually dismiss blocking Angular CDK overlays (Get code modal, etc.).
        // ensure_tab already does this, but `dismiss` clears a stuck page without
        // re-checking the URL/hydration.
        "dismiss" => {
            ai.dismiss_dialogs().await;
            "dialogs dismissed".into()
        }

        "state" => {
            let s = ai.get_state().await;
            format!(
                "url={} playground={} input={} streaming={} turns={}/{} model={}",
                s.url,
                s.is_on_playground,
                s.has_input,
                s.is_streaming,
                s.user_turn_count,
                s.model_turn_count,
                s.current_model
            )
        }

        "send" => {
            if sub_arg.is_empty() {
                return Err("send requires a prompt text".into());
            }
            ai.send_prompt(sub_arg).await?;
            format!("dispatched: {}", sub_arg)
        }

        // ask = send + wait + extract in one shot (mirrors deepseek ask).
        "ask" => {
            if sub_arg.is_empty() {
                return Err("ask requires a prompt text".into());
            }
            ai.send_and_wait(sub_arg).await?
        }

        "extract" => {
            let r = ai.extract_response().await;
            if r.is_empty() {
                "(no response yet, use 'aistudio wait' first)".into()
            } else {
                r
            }
        }

        "wait" => {
            let ready = ai.wait_for_response(60).await?;
            if ready {
                let r = ai.extract_response().await;
                if r.is_empty() {
                    "response ready but empty".into()
                } else {
                    r
                }
            } else {
                "timeout waiting for response".into()
            }
        }

        // Full conversation extraction.
        "turns" => {
            let turns = ai.extract_turns().await;
            if turns.is_empty() {
                return Ok("(no turns found)".into());
            }
            let mut out = String::new();
            for t in &turns {
                out.push_str(&format!("[{}] {}\n\n", t.role.as_label(), t.content));
            }
            out
        }
        "conversation" => {
            let c = ai.extract_conversation_text().await;
            if c.is_empty() {
                "(empty conversation)".into()
            } else {
                c
            }
        }

        // System instructions.
        "system" => {
            if sub_arg.is_empty() {
                let s = ai.get_system_instructions().await;
                if s.is_empty() {
                    "(no system instructions)".into()
                } else {
                    s
                }
            } else {
                ai.set_system_instructions(sub_arg).await?;
                format!("system instructions set ({} chars)", sub_arg.len())
            }
        }

        // Tool toggles.
        "tool" => {
            let t = Tool::from_label(sub_arg).ok_or_else(|| {
                format!(
                    "unknown tool: {}. Use: search|code|function|maps|url|structured",
                    sub_arg
                )
            })?;
            let on = ai.toggle_tool(t).await?;
            format!("tool {} toggled (now={})", t.as_label(), on)
        }

        // Temperature.
        "temp" => {
            if sub_arg.is_empty() {
                match ai.get_temperature().await {
                    Some(v) => format!("temperature: {}", v),
                    None => "(no temperature slider found)".into(),
                }
            } else {
                let v: f64 = sub_arg
                    .parse()
                    .map_err(|_| "temp requires a number 0.0–2.0".to_string())?;
                ai.set_temperature(v).await?;
                format!("temperature → {}", v)
            }
        }

        // Reasoning / thinking content (kept separate from the thinking-level setter).
        "reasoning" => ai
            .extract_thinking()
            .await
            .unwrap_or_else(|| "(no reasoning surfaced)".into()),

        "runtime" => ai
            .last_response_runtime()
            .await
            .map(|r| format!("runtime: {}", r))
            .unwrap_or_else(|| "(no runtime pill)".into()),

        // Generation resilience: manually rerun the last turn when a reply
        // fails to come back. (`ask` does this automatically.)
        "rerun" => {
            ai.rerun_last_turn().await?;
            // Optionally wait for the rerun to finish if asked: `rerun wait`.
            if sub_arg == "wait" {
                match ai.wait_for_response_or_rerun(3).await {
                    Ok(r) => format!("rerun ok: {}", r),
                    Err(_) => "rerun dispatched but extraction failed (virtual scroll)".into(),
                }
            } else {
                "rerun dispatched".into()
            }
        }

        // Feedback on the latest response: up (Good) / down (Bad).
        "rate" => {
            let up = match sub_arg {
                "up" | "good" => true,
                "down" | "bad" => false,
                _ => return Err("rate requires: up|good|down|bad".into()),
            };
            ai.rate_response(up).await?;
            if up { "rated 👍" } else { "rated 👎" }.into()
        }

        // Share the current prompt: opens the share dialog and prints the link.
        "share" => {
            ai.share().await?;
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            match ai.get_share_link().await {
                Some(link) => link,
                None => "(share dialog opened; no link found)".into(),
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
            if sub_arg.is_empty() {
                return Err("thinking requires: low|medium|high".into());
            }
            let level = ThinkingLevel::from_label(sub_arg)
                .ok_or_else(|| "thinking level must be: low|medium|high".to_string())?;
            ai.set_thinking_level(level.as_label()).await?;
            format!("thinking level → {}", level.as_label())
        }

        "new" => {
            ai.new_chat().await?;
            "new chat".into()
        }

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
            if items.is_empty() {
                return Ok("(no history items)".into());
            }
            let mut out = String::new();
            for (i, it) in items.iter().enumerate() {
                out.push_str(&format!("{}. {} | {}\n", i + 1, it.name, it.time));
            }
            out
        }

        "open" => {
            if sub_arg.is_empty() {
                return Err("open requires a prompt name from history".into());
            }
            ai.open_history_prompt(sub_arg).await?;
            format!("opened: {}", sub_arg)
        }

        _ => {
            return Err(
                "subcommands: ensure dismiss state send ask extract wait turns conversation system tool temp reasoning runtime rerun rate share model thinking new title code history open"
                    .into(),
            );
        }
    })
}
