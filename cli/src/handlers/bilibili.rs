//! `bilibili` — bilibili.com search & video details.

use crate::types::{CmdResult, kimi, split_arg};
use bilibili::{BilibiliSemantics, SortOrder};

pub async fn handle(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let bi = BilibiliSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { bi.ensure_tab().await?; "ok".into() }
        "search" => {
            if sub_arg.is_empty() { return Err("search requires keyword".into()); }
            bi.search(sub_arg).await?;
            format!("searched: {}", sub_arg)
        }
        "results" => {
            let n: usize = sub_arg.parse().unwrap_or(5);
            let results = bi.extract_results(n).await;
            let mut out = String::new();
            for (i, r) in results.iter().enumerate() {
                let ad = if r.is_ad { " [AD]" } else { "" };
                out.push_str(&format!("{}. {} | {} | {}{}\n  {}\n",
                    i+1, r.title, r.duration, r.uploader, ad, r.url));
            }
            if results.is_empty() { out.push_str("(no results)\n"); }
            out
        }
        "page" => {
            let n: u32 = sub_arg.parse().unwrap_or(1);
            bi.go_to_page(n).await?;
            format!("page → {}", n)
        }
        "sort" => {
            let order = match sub_arg {
                "relevance" => SortOrder::Relevance,
                "played" => SortOrder::MostPlayed,
                "newest" => SortOrder::Newest,
                "danmaku" => SortOrder::MostDanmaku,
                "favorited" => SortOrder::MostFavorited,
                _ => return Err("sort requires: relevance|played|newest|danmaku|favorited".into()),
            };
            bi.sort_by(order).await?;
            format!("sorted by {}", order.as_label())
        }
        "details" => {
            let url: Option<&str> = if sub_arg.is_empty() { None } else { Some(sub_arg) };
            let d = bi.extract_video_details(url).await?;
            let mut out = String::new();
            out.push_str(&format!("title: {}\nurl:   {}\n", d.title, d.url));
            if !d.views.is_empty() { out.push_str(&format!("views: {}\n", d.views)); }
            if !d.likes.is_empty() { out.push_str(&format!("likes: {}\n", d.likes)); }
            if !d.upload_date.is_empty() { out.push_str(&format!("date:  {}\n", d.upload_date)); }
            if !d.uploader.is_empty() { out.push_str(&format!("up:    {}\n", d.uploader)); }
            if !d.tags.is_empty() { out.push_str(&format!("tags:  {}\n", d.tags.join(", "))); }
            if !d.description.is_empty() { out.push_str(&format!("desc:  {}\n", &d.description[..d.description.len().min(300)])); }
            out
        }
        _ => return Err("subcommands: ensure search results page sort details".into()),
    })
}
