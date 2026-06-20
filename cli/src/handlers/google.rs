//! `google` — google.com search operations.

use crate::types::{CmdResult, kimi, split_arg, truncate};
use google::GoogleSemantics;

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let g = GoogleSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { g.ensure_tab().await?; "ok".into() }

        "search" => {
            if sub_arg.is_empty() { return Err("search requires query. Optional: 'search <mode> <query>' (mode: all,images,videos,news,shopping,forums,books,web,ai,short_videos)".into()); }
            // Check if first word is a mode
            let (mode_or_query, rest) = split_arg(sub_arg);
            let modes = ["all","ai","images","videos","news","shopping","forums","books","web","short_videos"];
            let (mode, query) = if modes.contains(&mode_or_query) {
                (mode_or_query, rest)
            } else {
                ("all", sub_arg)
            };
            if query.is_empty() { return Err("search requires a query".into()); }
            g.search_mode(query, mode).await?;
            let stats = g.result_stats().await;
            let results = g.extract_results(10).await;
            let mut out = format!("[mode: {}] {}\n", mode, stats);
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n  {}\n\n",
                    i+1, r.title, r.url, truncate(&r.snippet, 150)));
            }
            if results.is_empty() { out.push_str("(no results)\n"); }
            out
        }

        "mode" => {
            // Switch search mode on current results page by clicking tab
            if sub_arg.is_empty() { return Err("mode requires: ai,images,videos,news,shopping,forums,books,web,short_videos".into()); }
            let query = g.current_query().await;
            if query.is_empty() { return Err("no active search. Use 'google search <query>' first".into()); }
            g.search_mode(&query, sub_arg).await?;
            let stats = g.result_stats().await;
            let results = g.extract_results(10).await;
            let mut out = format!("[mode: {}] {}\n", sub_arg, stats);
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n  {}\n\n",
                    i+1, r.title, r.url, truncate(&r.snippet, 150)));
            }
            if results.is_empty() { out.push_str("(no results)\n"); }
            out
        }

        "recent" => {
            if sub_arg.is_empty() { return Err("recent requires query (e.g. 'recent Rust lang h' for past hour)".into()); }
            let parts: Vec<&str> = sub_arg.rsplitn(2, ' ').collect();
            if parts.len() < 2 { return Err("usage: google recent <query> <h|d|w|m|y>".into()); }
            let period = parts[0];
            let query = parts[1];
            let valid = ["h","d","w","m","y"];
            if !valid.contains(&period) { return Err(format!("invalid period '{}'. Use h/d/w/m/y", period).into()); }
            g.search_with_time(query, period).await?;
            let stats = g.result_stats().await;
            let results = g.extract_results(10).await;
            let mut out = format!("{}\n", stats);
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n  {}\n\n",
                    i+1, r.title, r.url, truncate(&r.snippet, 150)));
            }
            if results.is_empty() { out.push_str("(no results)\n"); }
            out
        }

        "next" => {
            g.next_page().await?;
            let stats = g.result_stats().await;
            let results = g.extract_results(10).await;
            let mut out = format!("{}\n", stats);
            for (i, r) in results.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n\n",
                    i+1, r.title, r.url));
            }
            out
        }

        "snippet" => {
            let text = g.extract_featured_snippet().await;
            if text.is_empty() { "(no featured snippet)".into() }
            else { text }
        }

        "ai" => {
            let text = g.extract_ai_overview().await;
            if text.is_empty() { "(no AI overview)".into() }
            else { text }
        }

        "query" => {
            g.current_query().await
        }

        _ => return Err("subcommands: ensure search mode recent next snippet ai query".into()),
    })
}
