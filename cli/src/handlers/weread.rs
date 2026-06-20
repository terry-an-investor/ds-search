//! `weread` — weread.qq.com reading operations.

use crate::types::{CmdResult, kimi, split_arg};
use weread::WeReadSemantics;

pub async fn handle(session: String, arg: String) -> CmdResult {
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
