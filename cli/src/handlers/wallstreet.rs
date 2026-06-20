//! `wallstreet` — wallstreetcn.com articles.

use crate::types::{CmdResult, kimi, split_arg};
use wallstreet::WallstreetSemantics;

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let ws = WallstreetSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => {
            ws.ensure_tab().await?;
            "ok".into()
        }
        "articles" => {
            let n: usize = sub_arg.parse().unwrap_or(5);
            let articles = ws.extract_articles(n).await;
            let mut out = String::new();
            for (i, a) in articles.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n", i + 1, a.title, a.url));
            }
            if articles.is_empty() {
                out.push_str("(no articles found)\n");
            }
            out
        }
        "search" => {
            if sub_arg.is_empty() {
                return Err("search requires keyword".into());
            }
            ws.search(sub_arg).await?;
            format!("searched: {}", sub_arg)
        }
        "body" => {
            let url: Option<&str> = if sub_arg.is_empty() {
                None
            } else {
                Some(sub_arg)
            };
            ws.extract_article_body(url).await?
        }
        _ => return Err("subcommands: ensure articles search body".into()),
    })
}
