//! `gemini` — gemini.google.com operations.

use crate::types::{CmdResult, kimi, split_arg};
use gemini::{GeminiModel, GeminiSemantics};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let gem = GeminiSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { gem.ensure_tab().await?; "ok".into() }
        "send" => {
            if sub_arg.is_empty() { return Err("send requires text".into()); }
            gem.ensure_tab().await?;
            gem.send_message(sub_arg).await?;
            "dispatched".into()
        }
        "extract" => {
            let r = gem.extract_last_response().await;
            if r.is_empty() { "(empty)".into() } else { r }
        }
        "thinking" => gem.extract_thinking().await.unwrap_or_else(|| "(none)".into()),
        "stream" => gem.get_streaming_state().await,
        "wait" => format!("response_ready: {}", gem.wait_for_response(30).await),
        "model" => {
            let m = match sub_arg {
                "fast" => GeminiModel::Fast,
                "thinking" => GeminiModel::Thinking,
                "pro" => GeminiModel::Pro,
                _ => return Err("model requires fast|thinking|pro".into()),
            };
            gem.select_model(m).await?;
            format!("model → {}", sub_arg)
        }
        "new" => { gem.new_conversation().await?; "ok".into() }
        _ => return Err("subcommands: ensure send extract thinking stream wait model new".into()),
    })
}
