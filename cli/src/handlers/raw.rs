//! `raw` — direct browser primitives for debugging.

use crate::types::{CmdResult, kimi, split_arg};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let k = kimi(&session);

    Ok(match sub {
        "eval" => {
            if sub_arg.is_empty() { return Err("raw eval requires JS code".into()); }
            let (v, _) = k.eval_js(sub_arg).await;
            v
        }
        "key" => {
            if sub_arg.is_empty() { return Err("raw key requires text".into()); }
            k.key_type(sub_arg).await?;
            "typed".into()
        }
        "enter" => { k.send_keys("Enter").await?; "enter sent".into() }
        "url" => k.get_url().await,
        "navigate" => {
            if sub_arg.is_empty() { return Err("raw navigate requires URL".into()); }
            k.navigate(sub_arg, false).await?;
            format!("navigated to {}", sub_arg)
        }
        _ => return Err("subcommands: eval key enter url navigate".into()),
    })
}
