//! ds — browser-automation CLI for web sites via Kimi WebBridge.
//!
//! Registry pattern: each command has an async handler registered in registry().
//! Add a new site: write handle_*() async fn, add 1 line to registry().

use bilibili::{BilibiliSemantics, SortOrder};
use deepseek::{ChatMode, DeepSeekSemantics, Feature};
use gemini::{GeminiModel, GeminiSemantics};
use grok::GrokSemantics;
use pilot::{KimiPrimitives, init_logging};
use wallstreet::{LiveCategory, LiveGlobalSemantics, WallstreetSemantics};
use weread::WeReadSemantics;
use google::GoogleSemantics;
use google_aistudio::{AistudioModel, AistudioSemantics};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

type CmdResult = Result<String, Box<dyn std::error::Error>>;
type CmdFuture = Pin<Box<dyn Future<Output = CmdResult> + Send>>;
type Handler = Box<dyn Fn(String, String) -> CmdFuture + Send + Sync>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();
    let mut session = "deepseek".to_string();
    let mut cmd_idx = 1;

    if args.len() > 2 && args[1] == "--session" {
        session = args[2].clone();
        cmd_idx = 3;
    }

    if args.len() <= cmd_idx {
        eprintln!("Usage: ds [--session <name>] <command> [args...]");
        eprintln!("Commands: {}", list_commands());
        std::process::exit(1);
    }

    let cmd = args[cmd_idx].clone();
    let arg = if args.len() > cmd_idx + 1 {
        args[cmd_idx + 1..].join(" ")
    } else {
        String::new()
    };

    match run_command(&cmd, &arg, &session).await {
        Ok(output) => { if !output.is_empty() { println!("{}", output); } }
        Err(e) => { eprintln!("error: {}", e); std::process::exit(1); }
    }
}

fn list_commands() -> String {
    registry().keys().map(|s| s.to_string()).collect::<Vec<_>>().join(" ")
}

async fn run_command(cmd: &str, arg: &str, session: &str) -> CmdResult {
    let handlers = registry();
    if let Some(handler) = handlers.get(cmd) {
        handler(session.to_string(), arg.to_string()).await
    } else {
        Err(format!("unknown command: {}. Available: {}", cmd, list_commands()).into())
    }
}

fn registry() -> HashMap<&'static str, Handler> {
    let mut m: HashMap<&'static str, Handler> = HashMap::new();
    m.insert("status",    Box::new(|s, a| Box::pin(handle_status(s, a))));
    m.insert("deepseek",  Box::new(|s, a| Box::pin(handle_deepseek(s, a))));
    m.insert("l2",        Box::new(|s, a| Box::pin(handle_deepseek(s, a))));
    m.insert("grok",      Box::new(|s, a| Box::pin(handle_grok(s, a))));
    m.insert("gemini",    Box::new(|s, a| Box::pin(handle_gemini(s, a))));
    m.insert("bilibili",  Box::new(|s, a| Box::pin(handle_bilibili(s, a))));
    m.insert("wallstreet",Box::new(|s, a| Box::pin(handle_wallstreet(s, a))));
    m.insert("livenews",  Box::new(|s, a| Box::pin(handle_livenews(s, a))));
    m.insert("weread",    Box::new(|s, a| Box::pin(handle_weread(s, a))));
    m.insert("google",    Box::new(|s, a| Box::pin(handle_google(s, a))));
    m.insert("aistudio",  Box::new(|s, a| Box::pin(handle_aistudio(s, a))));
    m.insert("meta",      Box::new(|s, a| Box::pin(handle_meta(s, a))));
    m.insert("raw",       Box::new(|s, a| Box::pin(handle_raw(s, a))));
    m
}

fn split_arg(arg: &str) -> (&str, &str) {
    let mut parts = arg.splitn(2, ' ');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

fn kimi(session: &str) -> KimiPrimitives {
    KimiPrimitives::new("http://127.0.0.1:10086", session)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { return s; }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

// ════════════════════════════════════════════════════════
// Handlers
// ════════════════════════════════════════════════════════

async fn handle_status(session: String, _arg: String) -> CmdResult {
    let client = reqwest::Client::new();
    match client
        .post("http://127.0.0.1:10086/command")
        .json(&serde_json::json!({"action": "list_tabs", "args": {}, "session": session}))
        .send().await
    {
        Ok(resp) if resp.status().is_success() => Ok("connected".into()),
        Ok(resp) => Err(format!("HTTP {}", resp.status()).into()),
        Err(e) => Err(format!("not connected: {}", e).into()),
    }
}

async fn handle_deepseek(session: String, arg: String) -> CmdResult {
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
        _ => return Err("subcommands: state ensure send extract thinking toggle mode new error log button scroll".into()),
    })
}

async fn handle_grok(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let grok = GrokSemantics::new(kimi(&session));

    Ok(match sub {
        "state" => format!("{:?}", grok.get_state().await),
        "ensure" => {
            grok.ensure_tab().await?;
            let s = grok.get_state().await;
            format!("url={} ta={} conv={}", s.url, s.has_input, s.has_conversation)
        }
        "send" => {
            if sub_arg.is_empty() { return Err("send requires text".into()); }
            grok.ensure_tab().await?;
            grok.send_message(sub_arg).await?;
            "dispatched".into()
        }
        "extract" => {
            let r = grok.extract_last_response().await;
            if r.is_empty() { "(empty)".into() } else { r }
        }
        "wait" => format!("response_ready: {}", grok.wait_for_response(30).await),
        "new" => { grok.new_conversation().await?; "ok".into() }
        _ => return Err("subcommands: state ensure send extract wait new".into()),
    })
}

async fn handle_gemini(session: String, arg: String) -> CmdResult {
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

async fn handle_bilibili(session: String, arg: String) -> CmdResult {
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

async fn handle_wallstreet(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let ws = WallstreetSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { ws.ensure_tab().await?; "ok".into() }
        "articles" => {
            let n: usize = sub_arg.parse().unwrap_or(5);
            let articles = ws.extract_articles(n).await;
            let mut out = String::new();
            for (i, a) in articles.iter().enumerate() {
                out.push_str(&format!("{}. {}\n  {}\n", i+1, a.title, a.url));
            }
            if articles.is_empty() { out.push_str("(no articles found)\n"); }
            out
        }
        "search" => {
            if sub_arg.is_empty() { return Err("search requires keyword".into()); }
            ws.search(sub_arg).await?;
            format!("searched: {}", sub_arg)
        }
        "body" => {
            let url: Option<&str> = if sub_arg.is_empty() { None } else { Some(sub_arg) };
            ws.extract_article_body(url).await?
        }
        _ => return Err("subcommands: ensure articles search body".into()),
    })
}

async fn handle_livenews(session: String, arg: String) -> CmdResult {
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

async fn handle_weread(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let wr = WeReadSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { wr.ensure_tab().await?; "ok".into() }

        "search" => {
            if sub_arg.is_empty() { return Err("search requires keyword".into()); }
            wr.search_books(sub_arg).await?;
            let books = wr.get_search_results(10).await;
            let mut out = String::new();
            for (i, b) in books.iter().enumerate() {
                out.push_str(&format!("{}. {} — {}\n", i+1, b.title, b.author));
            }
            if books.is_empty() { out.push_str("(no books found)\n"); }
            out
        }

        "open" => {
            if sub_arg.is_empty() { return Err("open requires a book URL or ID".into()); }
            wr.open_book(sub_arg).await?;
            let title = wr.current_chapter_title().await;
            let bid = wr.current_book_id().await;
            format!("opened: {} (bookId={})\nchapter: {}", sub_arg, bid, title)
        }

        "info" => {
            if let Some(bi) = wr.get_book_info().await {
                format!("{} — {}\n  publisher: {}\n  rating: {}% ({} ratings)\n  price: ¥{:.2}\n  format: {}",
                    bi.title, bi.author, bi.publisher, bi.rating, bi.rating_count, bi.price, bi.format)
            } else {
                "no book loaded (use 'weread open <url>' first)".into()
            }
        }

        "chapters" => {
            let long_id = if sub_arg.is_empty() {
                let bid = wr.current_book_id().await;
                if bid.is_empty() { return Err("no book loaded. Use 'weread open <url>' first or 'weread chapters <long_book_id>'".into()); }
                // We need long ID for the API. Try to get it from readInfo.
                // Fall back to a mapping if possible.
                bid
            } else {
                sub_arg.to_string()
            };

            // Try fetching chapter list — the long ID might be different from URL short ID
            // Try the input as long ID first, then as short ID
            let chapters = wr.get_chapter_list(&long_id).await;
            if chapters.is_empty() {
                // Try with the URL's short ID conversion (common pattern)
                return Ok(format!("no chapters found for id={}. Try 'weread info' first to get the book id, then use the long id from the API.", long_id));
            }
            let mut out = format!("{} chapters\n", chapters.len());
            for ch in chapters.iter().take(50) {
                let level_indent = "  ".repeat((ch.level.saturating_sub(1)) as usize);
                let price_note = if ch.price > 0 { " [付费]" } else { "" };
                out.push_str(&format!("{}[{:02}] {}{} ({}字)\n",
                    level_indent, ch.chapter_idx, ch.title, price_note, ch.word_count));
            }
            if chapters.len() > 50 {
                out.push_str(&format!("... and {} more\n", chapters.len() - 50));
            }
            out
        }

        "read" => {
            wr.ensure_tab().await?;
            let text = wr.extract_page_text().await;
            if text.is_empty() { "no text available on current page".into() }
            else { text }
        }

        "highlights" => {
            wr.ensure_tab().await?;
            let text = wr.extract_highlights().await;
            if text.is_empty() { "(no highlights found)".into() }
            else { text }
        }

        "outline" => {
            wr.ensure_tab().await?;
            let text = wr.extract_ai_outline().await;
            if text.is_empty() { "(no AI outline found)".into() }
            else { text }
        }

        "next" => {
            // Ensure we're on a reader page
            wr.ensure_tab().await?;
            let url = wr.kimi.get_url().await;
            if !url.contains("/web/reader/") {
                return Err("not on a reader page. Use 'weread open <url>' first".into());
            }
            wr.next_chapter().await?;
            let title = wr.current_chapter_title().await;
            format!("→ {}", title)
        }

        "prev" => {
            wr.ensure_tab().await?;
            let url = wr.kimi.get_url().await;
            if !url.contains("/web/reader/") {
                return Err("not on a reader page. Use 'weread open <url>' first".into());
            }
            wr.prev_chapter().await?;
            let title = wr.current_chapter_title().await;
            format!("← {}", title)
        }

        "progress" => {
            wr.ensure_tab().await?;
            let pct = wr.reading_progress().await;
            let title = wr.current_chapter_title().await;
            format!("{} (chapter: {})", if pct.is_empty() { "unknown" } else { &pct }, title)
        }

        _ => return Err("subcommands: ensure search open info chapters read highlights outline next prev progress".into()),
    })
}

async fn handle_google(session: String, arg: String) -> CmdResult {
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

async fn handle_aistudio(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let ai = AistudioSemantics::new(kimi(&session));

    Ok(match sub {
        "ensure" => { ai.ensure_tab().await?; "ok".into() }

        "send" => {
            if sub_arg.is_empty() { return Err("send requires a prompt text".into()); }
            ai.send_prompt(sub_arg).await?;
            format!("dispatched: {}", sub_arg)
        }

        "extract" => {
            let r = ai.extract_response().await;
            if r.is_empty() { "(no response yet, use 'aistudio wait' first)".into() }
            else { r }
        }

        "wait" => {
            let ready = ai.wait_for_response(60).await?;
            if ready {
                let r = ai.extract_response().await;
                if r.is_empty() { "response ready but empty".into() }
                else { r }
            } else {
                "timeout waiting for response".into()
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
            if sub_arg.is_empty() { return Err("thinking requires: low|medium|high".into()); }
            let valid = ["low", "medium", "high"];
            if !valid.contains(&sub_arg.to_lowercase().as_str()) {
                return Err("thinking level must be: low|medium|high".into());
            }
            ai.set_thinking_level(&sub_arg.to_lowercase()).await?;
            format!("thinking level → {}", sub_arg.to_lowercase())
        }

        "new" => { ai.new_chat().await?; "new chat".into() }

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
            if items.is_empty() { return Ok("(no history items)".into()); }
            let mut out = String::new();
            for (i, it) in items.iter().enumerate() {
                out.push_str(&format!("{}. {} | {}\n", i + 1, it.name, it.time));
            }
            out
        }

        "open" => {
            if sub_arg.is_empty() { return Err("open requires a prompt name from history".into()); }
            ai.open_history_prompt(sub_arg).await?;
            format!("opened: {}", sub_arg)
        }

        _ => return Err("subcommands: ensure send extract wait model thinking new title code history open".into()),
    })
}

async fn handle_raw(session: String, arg: String) -> CmdResult {
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

async fn handle_meta(session: String, arg: String) -> CmdResult {
    let (sub, sub_arg) = split_arg(&arg);
    let k = kimi(&session);

    match sub {
        "scan" => do_meta_scan(&k).await,
        "click" => do_meta_click(&k, sub_arg).await,
        "url" => Ok(k.get_url().await),
        "save" => do_meta_save(&k, sub_arg).await,
        "diff" => do_meta_diff(&k, sub_arg).await,
        "watch" => do_meta_watch(&k, sub_arg).await,
        "response" => do_meta_response(&k, sub_arg).await,
        _ => Err("meta subcommands: scan click url save diff watch response".into()),
    }
}

// ════════════════════════════════════════════════════════
// Meta helper functions
// ════════════════════════════════════════════════════════

async fn do_meta_scan(kimi: &KimiPrimitives) -> CmdResult {
    let (raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{
            const vh=window.innerHeight;
            const inputs=Array.from(document.querySelectorAll(
                'textarea, input:not([type=hidden]), [contenteditable=true]'
            )).map(function(e){
                const r=e.getBoundingClientRect();
                const bottomDist=vh-r.bottom;
                const nearBottom=bottomDist<200&&r.top>vh*0.3;
                return {
                    tag:e.tagName, type:e.type||'', placeholder:e.placeholder||'',
                    value:(e.value||e.textContent||'').substring(0,80), disabled:e.disabled,
                    nearBottom:nearBottom, w:Math.round(r.width), h:Math.round(r.height)
                };
            });
            inputs.sort(function(a,b){
                if(a.nearBottom&&!b.nearBottom)return -1;
                if(!a.nearBottom&&b.nearBottom)return 1;
                return (b.w*b.h)-(a.w*a.h);
            });
            const allButtons=Array.from(document.querySelectorAll(
                'button, [role=button], [role=radio], [role=switch], [role=tab]'
            )).map(function(e){return{
                text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),
                role:e.getAttribute('role')||e.tagName.toLowerCase(),
                checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',
                disabled:e.disabled,
                parentClass:(e.parentElement?.className||'').split(' ').slice(0,3).join(' ')
            }});
            const buttons=allButtons.filter(function(b){return b.text.length>0});
            const spinnerDetails=[
                {selector:'.ds-loading',count:document.querySelectorAll('.ds-loading').length},
                {selector:'[aria-busy=true]',count:document.querySelectorAll('[aria-busy=true]').length},
                {selector:'[class*=spinner]',count:document.querySelectorAll('[class*=spinner]').length},
                {selector:'[class*=loading]',count:document.querySelectorAll('[class*=loading]').length}
            ];
            return {
                url:location.href, title:document.title,
                inputs:inputs, buttons:buttons, totalButtons:allButtons.length,
                spinners:spinnerDetails,
                bodySnippet:document.body?document.body.innerText.substring(0,500):''
            };
        })())"#,
    ).await;

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("url:    {}\n", v.get("url").and_then(|s|s.as_str()).unwrap_or("")));
    out.push_str(&format!("title:  {}\n", v.get("title").and_then(|s|s.as_str()).unwrap_or("")));

    if let Some(spinners) = v.get("spinners").and_then(|a|a.as_array()) {
        let active: Vec<_> = spinners.iter().filter(|s|
            s.get("count").and_then(|c|c.as_u64()).unwrap_or(0) > 0
        ).collect();
        out.push_str(&format!("spinners: {} selectors matched ({} elements total)\n",
            active.len(), active.iter().map(|s| s.get("count").and_then(|c|c.as_u64()).unwrap_or(0)).sum::<u64>()));
        for s in &active {
            out.push_str(&format!("  {} ×{}\n",
                s.get("selector").and_then(|v|v.as_str()).unwrap_or(""),
                s.get("count").and_then(|c|c.as_u64()).unwrap_or(0)));
        }
    }

    if let Some(inputs) = v.get("inputs").and_then(|a|a.as_array()) {
        out.push_str(&format!("inputs ({}):\n", inputs.len()));
        for inp in inputs {
            let near = inp.get("nearBottom").and_then(|b|b.as_bool()).unwrap_or(false);
            let star = if near { " ★" } else { "" };
            out.push_str(&format!("  [{}] placeholder='{}' disabled={}{}\n",
                inp.get("tag").and_then(|s|s.as_str()).unwrap_or(""),
                inp.get("placeholder").and_then(|s|s.as_str()).unwrap_or(""),
                inp.get("disabled").and_then(|b|b.as_bool()).unwrap_or(false),
                star,
            ));
        }
    }
    if let Some(buttons) = v.get("buttons").and_then(|a|a.as_array()) {
        out.push_str(&format!("buttons ({}):\n", buttons.len()));
        for b in buttons {
            out.push_str(&format!("  [{}] '{}' checked={} disabled={}\n",
                b.get("role").and_then(|s|s.as_str()).unwrap_or(""),
                b.get("text").and_then(|s|s.as_str()).unwrap_or(""),
                b.get("checked").and_then(|s|s.as_str()).unwrap_or("-"),
                b.get("disabled").and_then(|b|b.as_bool()).unwrap_or(false),
            ));
        }
    }
    if v.get("totalButtons").and_then(|n|n.as_u64()).unwrap_or(0) > 20 {
        out.push_str("  (many icon-only buttons hidden)\n");
    }
    out.push_str(&format!("body:   {}\n",
        v.get("bodySnippet").and_then(|s|s.as_str()).unwrap_or("").chars().take(300).collect::<String>()
    ));
    Ok(out)
}

async fn do_meta_click(kimi: &KimiPrimitives, text: &str) -> CmdResult {
    if text.is_empty() { return Err("meta click requires text to find".into()); }
    let (raw, _) = kimi.eval_js(&format!(
        r#"JSON.stringify((()=>{{
            const els=Array.from(document.querySelectorAll(
                'button,[role=button],[role=radio],[role=tab],a,[role=link]'
            ));
            const t=els.find(function(e){{return (e.textContent||'').includes('{}');}});
            if(!t)return {{found:false,samples:els.slice(0,10).map(function(e){{return(e.textContent||'').trim().substring(0,40)}})}};
            t.click();
            return {{found:true,text:(t.textContent||'').trim().substring(0,60),tag:t.tagName,parentClass:(t.parentElement?.className||'').split(' ').slice(0,5).join(' '),href:t.getAttribute('href')||''}};
        }})())"#,
        text
    )).await;
    Ok(raw)
}

async fn do_meta_save(kimi: &KimiPrimitives, name: &str) -> CmdResult {
    if name.is_empty() { return Err("meta save requires a name".into()); }
    let safe_name = name.replace(['/', '\\', '.'], "_");
    let dir = std::path::Path::new("knowledge/scans");
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", safe_name));
    let (scan_raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{const b=Array.from(document.querySelectorAll('button,[role=button],[role=radio],[role=switch],[role=tab]')).map(function(e){return{text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),role:e.getAttribute('role')||e.tagName.toLowerCase(),checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',disabled:e.disabled}}).filter(function(b){return b.text.length>0});const i=Array.from(document.querySelectorAll('textarea,input:not([type=hidden]),[contenteditable=true]')).map(function(e){return{tag:e.tagName,type:e.type||'',placeholder:e.placeholder||'',disabled:e.disabled}});const dynEls=Array.from(document.querySelectorAll('[class*=response],[class*=message],[class*=turn],[class*=thought]')).map(function(e){return{cls:e.className.split(' ').slice(0,2).join(' '),len:(e.textContent||'').length}});return JSON.stringify({url:location.href,title:document.title,inputs:i,buttons:b,bodySnippet:(document.body?.innerText||'').substring(0,2000),dynEls:dynEls,timestamp:Date.now()})})())"#,
    ).await;
    let parsed: serde_json::Value = serde_json::from_str(&scan_raw).unwrap_or(serde_json::Value::String(scan_raw.clone()));
    let to_save = if parsed.is_string() { parsed.as_str().unwrap_or(&scan_raw).to_string() } else { scan_raw };
    let pretty: serde_json::Value = serde_json::from_str(&to_save)?;
    std::fs::write(&path, serde_json::to_string_pretty(&pretty)?)?;
    Ok(format!("saved to {}", path.display()))
}

async fn do_meta_diff(kimi: &KimiPrimitives, name: &str) -> CmdResult {
    if name.is_empty() { return Err("meta diff requires a saved snapshot name".into()); }
    let safe_name = name.replace(['/', '\\', '.'], "_");
    let path = std::path::Path::new("knowledge/scans").join(format!("{}.json", safe_name));
    let old: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)
        .map_err(|_| format!("snapshot '{}' not found at {}", name, path.display()))?)?;
    let (new_raw, _) = kimi.eval_js(
        r#"JSON.stringify((()=>{const b=Array.from(document.querySelectorAll('button,[role=button],[role=radio],[role=switch],[role=tab]')).map(function(e){return{text:(e.textContent||'').trim().replace(/\s+/g,' ').substring(0,60),role:e.getAttribute('role')||e.tagName.toLowerCase(),checked:e.getAttribute('aria-checked')||e.getAttribute('aria-pressed')||'',disabled:e.disabled}}).filter(function(b){return b.text.length>0});const i=Array.from(document.querySelectorAll('textarea,input:not([type=hidden]),[contenteditable=true]')).map(function(e){return{tag:e.tagName,type:e.type||'',placeholder:e.placeholder||'',disabled:e.disabled}});const dynEls=Array.from(document.querySelectorAll('[class*=response],[class*=message],[class*=turn],[class*=thought]')).map(function(e){return{cls:e.className.split(' ').slice(0,2).join(' '),len:(e.textContent||'').length}});return JSON.stringify({url:location.href,title:document.title,inputs:i,buttons:b,bodySnippet:(document.body?.innerText||'').substring(0,2000),dynEls:dynEls,timestamp:Date.now()})})())"#,
    ).await;
    let new_parsed: serde_json::Value = serde_json::from_str(&new_raw).unwrap_or(serde_json::Value::String(new_raw.clone()));
    let new_str = if new_parsed.is_string() { new_parsed.as_str().unwrap_or(&new_raw).to_string() } else { new_raw };
    let new: serde_json::Value = serde_json::from_str(&new_str)?;
    let mut out = String::new();
    out.push_str(&format!("diff '{}' vs live:\n", name));
    let old_url = old.get("url").and_then(|s|s.as_str()).unwrap_or("");
    let new_url = new.get("url").and_then(|s|s.as_str()).unwrap_or("");
    let old_btns = old.get("buttons").and_then(|a|a.as_array()).cloned().unwrap_or_default();
    let new_btns = new.get("buttons").and_then(|a|a.as_array()).cloned().unwrap_or_default();
    let old_set: std::collections::HashSet<String> = old_btns.iter()
        .filter_map(|b| b.get("text").and_then(|s|s.as_str()).map(|s| s.to_string())).collect();
    let new_set: std::collections::HashSet<String> = new_btns.iter()
        .filter_map(|b| b.get("text").and_then(|s|s.as_str()).map(|s| s.to_string())).collect();
    let added: Vec<_> = new_set.difference(&old_set).collect();
    let removed: Vec<_> = old_set.difference(&new_set).collect();
    let old_dyn = old.get("dynEls").and_then(|a|a.as_array()).cloned().unwrap_or_default();
    let new_dyn = new.get("dynEls").and_then(|a|a.as_array()).cloned().unwrap_or_default();
    let old_body = old.get("bodySnippet").and_then(|s|s.as_str()).unwrap_or("");
    let new_body = new.get("bodySnippet").and_then(|s|s.as_str()).unwrap_or("");

    let mut changes = 0;
    if old_url != new_url { out.push_str(&format!("  URL: {} → {}\n", old_url, new_url)); changes += 1; }
    if !added.is_empty() { out.push_str(&format!("  + added: {}\n", added.iter().map(|s| format!("'{}'", s)).collect::<Vec<_>>().join(", "))); changes += 1; }
    if !removed.is_empty() { out.push_str(&format!("  - removed: {}\n", removed.iter().map(|s| format!("'{}'", s)).collect::<Vec<_>>().join(", "))); changes += 1; }
    for nb in &new_btns {
        let nt = nb.get("text").and_then(|s|s.as_str()).unwrap_or("");
        let nc = nb.get("checked").and_then(|s|s.as_str()).unwrap_or("");
        if let Some(ob) = old_btns.iter().find(|b| b.get("text").and_then(|s|s.as_str()) == Some(nt)) {
            let oc = ob.get("checked").and_then(|s|s.as_str()).unwrap_or("");
            if oc != nc && !oc.is_empty() { out.push_str(&format!("  ~ '{}' checked: {} → {}\n", nt, oc, nc)); changes += 1; }
        }
    }
    if new_dyn.len() != old_dyn.len() {
        out.push_str(&format!("  Δ dynamic elements: {} → {}\n", old_dyn.len(), new_dyn.len()));
        changes += 1;
        for nd in &new_dyn {
            let nc = nd.get("cls").and_then(|s|s.as_str()).unwrap_or("");
            let nl = nd.get("len").and_then(|n|n.as_u64()).unwrap_or(0);
            let found = old_dyn.iter().any(|od|
                od.get("cls").and_then(|s|s.as_str()) == Some(nc) &&
                od.get("len").and_then(|n|n.as_u64()) == Some(nl)
            );
            if !found && nl > 0 { out.push_str(&format!("    new: {} ({} chars)\n", nc, nl)); }
        }
    }
    if old_body != new_body {
        let old_words: std::collections::HashSet<&str> = old_body.split(' ').collect();
        let new_words: std::collections::HashSet<&str> = new_body.split(' ').collect();
        let new_wc: Vec<_> = new_words.difference(&old_words).collect();
        if !new_wc.is_empty() && new_wc.len() < 30 {
            out.push_str(&format!("  Δ body: +{} new words\n", new_wc.len()));
            changes += 1;
        } else if new_body.len() > old_body.len() + 100 {
            out.push_str(&format!("  Δ body: {} → {} chars (+{})\n", old_body.len(), new_body.len(), new_body.len()-old_body.len()));
            changes += 1;
        }
    }
    if changes == 0 { out.push_str("  (no changes detected)\n"); }
    Ok(out)
}

async fn do_meta_watch(kimi: &KimiPrimitives, arg: &str) -> CmdResult {
    let interval_ms: u64 = arg.parse().unwrap_or(1000);
    let rounds: u64 = if arg.contains('x') {
        arg.split('x').nth(1).and_then(|s| s.parse().ok()).unwrap_or(10)
    } else { 10 };

    let mut out = String::new();
    let mut last_body = String::new();
    let mut last_url = String::new();

    for i in 0..rounds {
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        let (url, _) = kimi.eval_js("location.href").await;
        let (body, _) = kimi.eval_js("document.body?.innerText?.substring(0,300) || ''").await;

        let mut changes = Vec::new();
        if url != last_url && !last_url.is_empty() { changes.push(format!("URL: {} → {}", last_url, url)); }
        if body != last_body && !last_body.is_empty() {
            let delta = body.len() as i64 - last_body.len() as i64;
            if delta != 0 { changes.push(format!("body: {}{} chars", if delta>0{"+"}else{""}, delta)); }
        }

        if !changes.is_empty() || i == 0 {
            out.push_str(&format!("[{}ms] {}\n", i*interval_ms, changes.join(" | ")));
        }
        if i == 0 || !changes.is_empty() {
            out.push_str(&format!("  url={}\n  body={}\n", &url[..url.len().min(80)], &body[..body.len().min(120)]));
        }

        last_url = url;
        last_body = body;
    }
    Ok(out)
}

async fn do_meta_response(kimi: &KimiPrimitives, _arg: &str) -> CmdResult {
    let (before, _) = kimi.eval_js("document.body?.innerText?.substring(0,3000) || ''").await;
    let before_count = before.len();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut last_len = before_count;
    let mut stable = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let (body, _) = kimi.eval_js("document.body?.innerText?.substring(0,5000) || ''").await;
        let len = body.len();
        if len == last_len {
            stable += 1;
            if stable >= 4 && len > before_count { break; }
        } else {
            last_len = len;
            stable = 0;
        }
        if tokio::time::Instant::now() > deadline { break; }
    }

    let (after, _) = kimi.eval_js("document.body?.innerText?.substring(0,5000) || ''").await;
    let mut out = String::new();
    out.push_str(&format!("body: {} → {} chars (+{})\n", before_count, after.len(), after.len().saturating_sub(before_count)));

    if after.len() > before_count + 20 {
        let new_text = &after[before_count.min(after.len())..];
        out.push_str(&format!("new content:\n---\n{}\n---\n", &new_text[..new_text.len().min(1000)]));
    } else if after.len() <= before_count {
        out.push_str("(no new content detected)\n");
    }
    Ok(out)
}
