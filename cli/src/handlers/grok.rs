//! `grok` — x.com/i/grok operations.

use crate::types::{CmdResult, kimi, split_arg};
use grok::GrokSemantics;

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let grok = GrokSemantics::new(kimi(&session));

    Ok(match sub {
        "state" => format!("{:?}", grok.get_state().await),
        "ensure" => {
            grok.ensure_tab().await?;
            let s = grok.get_state().await;
            format!(
                "url={} ta={} conv={}",
                s.url, s.has_input, s.has_conversation
            )
        }
        "send" => {
            if sub_arg.is_empty() {
                return Err("send requires text".into());
            }
            grok.ensure_tab().await?;
            grok.send_message(sub_arg).await?;
            "dispatched".into()
        }
        "extract" => {
            let r = grok.extract_last_response().await;
            if r.is_empty() { "(empty)".into() } else { r }
        }
        "wait" => format!("response_ready: {}", grok.wait_for_response(30).await),
        "new" => {
            grok.new_conversation().await?;
            "ok".into()
        }
        _ => return Err("subcommands: state ensure send extract wait new".into()),
    })
}
