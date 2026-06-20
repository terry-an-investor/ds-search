//! `livenews` — wallstreetcn.com/live/global real-time newsfeed.

use crate::types::{CmdResult, kimi, split_arg, truncate};
use wallstreet::{LiveCategory, LiveGlobalSemantics};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let live = LiveGlobalSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { live.ensure_tab().await?; "ok".into() }
        "items" => {
            let n: usize = sub_arg.parse().unwrap_or(10);
            let items = live.extract_items(n).await;
            let mut out = String::new();
            for it in &items {
                out.push_str(&format!("[{}] {}\n  {}\n",
                    it.time, it.title, truncate(&it.content, 120)));
            }
            if items.is_empty() { out.push_str("(no live items)\n"); }
            out
        }
        "category" => {
            if sub_arg.is_empty() {
                let cat = live.current_category().await;
                return Ok(format!("current: {:?}", cat));
            }
            let cat = LiveCategory::from_label(sub_arg)
                .ok_or_else(|| format!("unknown category: {}. Use: 要闻|A股|美股|港股|外汇|商品|债券|科技", sub_arg))?;
            live.switch_category(cat).await?;
            format!("category → {}", cat.as_label())
        }
        "important" => {
            format!("important_only: {}", live.toggle_important_only().await?)
        }
        "time" => live.header_time().await,
        "poll" => {
            let last_count: usize = sub_arg.parse().unwrap_or(0);
            let new_items = live.poll_new_items(last_count).await;
            let mut out = format!("{} new items\n", new_items.len());
            for it in &new_items {
                out.push_str(&format!("[{}] {}\n", it.time, it.title));
            }
            out
        }
        _ => return Err("subcommands: ensure items category important time poll".into()),
    })
}
