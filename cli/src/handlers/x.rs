//! `x` — x.com tweet thread operations.

use crate::types::{CmdResult, kimi, split_arg};
use x::XSemantics;

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let x_sem = XSemantics::new(kimi(&session));

    Ok(match sub {
        "thread" => {
            if sub_arg.is_empty() {
                return Err("thread requires a tweet URL".into());
            }
            x_sem.navigate_to_tweet(sub_arg).await?;
            x_sem.scroll_to_load_replies(10).await?;
            let thread = x_sem.extract_thread().await?;
            let mut out = String::new();
            out.push_str(&format!("Main tweet by @{}:\n", thread.main_tweet.author));
            out.push_str(&format!("{}\n", thread.main_tweet.text));
            out.push_str(&format!("URL: {}\n", thread.main_tweet.url));
            if !thread.main_tweet.external_links.is_empty() {
                out.push_str("External links:\n");
                for link in &thread.main_tweet.external_links {
                    out.push_str(&format!("  - {}\n", link.url));
                }
            }
            out.push_str(&format!("\n{} replies in thread:\n", thread.replies.len()));
            for (i, reply) in thread.replies.iter().enumerate() {
                out.push_str(&format!("\n[{}] @{}:\n", i + 1, reply.author));
                out.push_str(&format!("{}\n", reply.text));
                if !reply.external_links.is_empty() {
                    out.push_str("External links:\n");
                    for link in &reply.external_links {
                        out.push_str(&format!("  - {}\n", link.url));
                    }
                }
            }
            out
        }

        "links" => {
            if sub_arg.is_empty() {
                return Err("links requires a tweet URL".into());
            }
            x_sem.navigate_to_tweet(sub_arg).await?;
            x_sem.scroll_to_load_replies(10).await?;
            let links = x_sem.extract_external_links().await;
            if links.is_empty() {
                "(no external links found)".into()
            } else {
                let mut out = format!("{} external links:\n", links.len());
                for link in &links {
                    out.push_str(&format!("- {}\n", link.url));
                }
                out
            }
        }

        _ => return Err("subcommands: thread links".into()),
    })
}
