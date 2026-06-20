//! WeReadSemantics — weread.qq.com digital reading operations.
//!
//! Features: search, book info, chapter list, read, navigate.
//! Content is Canvas-rendered; text extracted from hidden note panels + APIs.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::{BookInfo, ChapterInfo};

const WEREAD_URL: &str = "https://weread.qq.com";

#[derive(Debug, Clone)]
pub struct WeReadSemantics {
    pub kimi: KimiPrimitives, // pub for URL check in CLI handler
}

impl WeReadSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ═══════════════════════════════════════════════════════════
    // Tab management
    // ═══════════════════════════════════════════════════════════

    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("weread.qq.com") {
            self.kimi.navigate(WEREAD_URL, false).await?;
        }
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Navigate to the homepage.
    pub async fn go_home(&self) -> Result<()> {
        self.kimi.navigate(WEREAD_URL, false).await?;
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Open a book reader by book URL (e.g. https://weread.qq.com/web/reader/<id>).
    pub async fn open_book(&self, url_or_id: &str) -> Result<()> {
        let url = if url_or_id.starts_with("http") || url_or_id.contains("weread.qq.com") {
            url_or_id.to_string()
        } else {
            format!("https://weread.qq.com/web/reader/{}", url_or_id)
        };
        self.kimi.navigate(&url, false).await?;
        // Wait for SPA to load
        for _ in 0..30 {
            let (ready, _) = self
                .kimi
                .eval_js("!!(document.querySelector('.readerTopBar_title_chapter'))")
                .await;
            if ready == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Search
    // ═══════════════════════════════════════════════════════════

    /// Search for books by keyword. Navigates to search results page.
    pub async fn search_books(&self, keyword: &str) -> Result<()> {
        self.go_home().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Focus search input and type
        let _ = self.kimi.eval_js(
            "(function(){var i=document.querySelector('input[type=text],input[placeholder*=搜索],input[placeholder*=书]');if(i){i.focus();i.value=''}})()"
        ).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.kimi.key_type(keyword).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.kimi.send_keys("Enter").await?;

        // Wait for search results
        for _ in 0..20 {
            let (count, _) = self
                .kimi
                .eval_js("document.querySelectorAll('a[href*=\"/web/reader/\"]').length")
                .await;
            let n: usize = count.parse().unwrap_or(0);
            if n > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Get search result book links from current page.
    pub async fn get_search_results(&self, max: usize) -> Vec<BookInfo> {
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify(Array.from(document.querySelectorAll('a[href*="/web/reader/"]')).slice(0,{}).map(function(a){{
                var t=a.textContent.trim().replace(/\\s+/g,' ');
                return {{
                    title:t.substring(0,120),
                    url:a.getAttribute('href'),
                    author:''
                }};
            }}))"#,
            max
        )).await;
        parse_book_links(&raw)
    }

    // ═══════════════════════════════════════════════════════════
    // Book info
    // ═══════════════════════════════════════════════════════════

    /// Get current book info from __INITIAL_STATE__.
    pub async fn get_book_info(&self) -> Option<BookInfo> {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var s=window.__INITIAL_STATE__;if(!s||!s.reader||!s.reader.bookInfo)return'{}';var bi=s.reader.bookInfo;return JSON.stringify({bookId:s.reader.bookId||'',title:bi.title||'',author:bi.author||'',cover:bi.cover||'',publisher:bi.publisher||'',format:bi.format||'',price:bi.price||0,rating:bi.star||0,ratingCount:bi.ratingCount||0,intro:'',totalChapters:0})})()"
        ).await;
        parse_book_info(&raw)
    }

    /// Get full book info via readInfo API (includes intro, chapter count).
    pub async fn get_book_info_api(&self, long_book_id: &str) -> Option<BookInfo> {
        let js = format!(
            r#"(async function(){{
                try{{
                    var r=await fetch('https://weread.qq.com/web/book/readInfo?bookId={}',{{method:'GET',credentials:'include'}});
                    var d=await r.json();
                    var bi=d.bookInfo||{{}};
                    return JSON.stringify({{
                        bookId:bi.bookId||'',
                        longBookId:bi.bookId||'',
                        title:bi.title||'',
                        author:bi.author||'',
                        cover:bi.cover||'',
                        publisher:bi.publisher||'',
                        format:bi.format||'',
                        price:bi.centPrice?bi.centPrice/100:0,
                        rating:bi.newRating?bi.newRating*20:0,
                        ratingCount:bi.newRatingCount||0,
                        intro:bi.intro||'',
                        totalChapters:bi.lastChapterIdx?bi.lastChapterIdx+1:0
                    }});
                }}catch(e){{return JSON.stringify({{error:e.toString()}});}}
            }})()"#,
            long_book_id
        );
        let (raw, _) = self.kimi.eval_js(&js).await;
        parse_book_info(&raw)
    }

    // ═══════════════════════════════════════════════════════════
    // Chapter list
    // ═══════════════════════════════════════════════════════════

    /// Fetch chapter list via chapterInfos API.
    pub async fn get_chapter_list(&self, long_book_id: &str) -> Vec<ChapterInfo> {
        let js = format!(
            r#"(async function(){{
                try{{
                    var r=await fetch('https://weread.qq.com/web/book/chapterInfos',{{
                        method:'POST',
                        credentials:'include',
                        headers:{{'Content-Type':'application/json'}},
                        body:JSON.stringify({{bookIds:['{}']}})
                    }});
                    var d=await r.json();
                    var chs=(d.data&&d.data[0]&&d.data[0].updated)?d.data[0].updated:[];
                    return JSON.stringify(chs.map(function(c){{return{{
                        chapterUid:c.chapterUid,chapterIdx:c.chapterIdx,
                        title:c.title||'',wordCount:c.wordCount||0,
                        level:c.level||1,price:c.price||0,paid:c.paid||0,tar:c.tar||''
                    }};}}));
                }}catch(e){{return JSON.stringify([]);}}
            }})()"#,
            long_book_id
        );
        let (raw, _) = self.kimi.eval_js(&js).await;
        parse_chapter_list(&raw)
    }

    // ═══════════════════════════════════════════════════════════
    // Reading — extract text from current page
    // ═══════════════════════════════════════════════════════════

    /// Get the current chapter title from the top bar.
    pub async fn current_chapter_title(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var el=document.querySelector('.readerTopBar_title_chapter');return el?el.textContent.trim():''})()"
        ).await;
        raw.trim().to_string()
    }

    /// Extract available text from the reader page.
    ///
    /// Since content is Canvas-rendered, we extract from hidden panels:
    /// - Reader note panel (popular highlights/annotations)
    /// - AI outline (book structure + key points)
    /// - Chapter catalog (TOC)
    pub async fn extract_page_text(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){\
                var parts=[];\
                var ch=document.querySelector('.readerTopBar_title_chapter');\
                if(ch)parts.push('## '+ch.textContent.trim());\
                var notes=document.querySelector('.readerNoteList');\
                if(notes){var t=notes.innerText.trim();if(t.length>0)parts.push('--- 热门划线 ---\\n'+t)}\
                var outline=document.querySelector('.wr_outline_book_detail_main');\
                if(outline){var t=outline.innerText.trim();if(t.length>0)parts.push('--- AI大纲 ---\\n'+t)}\
                var catalog=document.querySelector('.readerCatalog');\
                if(catalog){var t=catalog.innerText.trim();if(t.length>0)parts.push('--- 目录 ---\\n'+t)}\
                var meta=document.querySelector('meta[name=\"description\"]');\
                if(meta){var c=meta.getAttribute('content');if(c&&c.length>0)parts.push('--- 简介 ---\\n'+c)}\
                return parts.join('\\n\\n');\
            })()"
        ).await;
        raw
    }

    /// Extract only the popular highlights/notes from the current page.
    /// This is the richest text source for getting actual book content.
    pub async fn extract_highlights(&self) -> String {
        let (raw, _) = self
            .kimi
            .eval_js(
                "(function(){\
                var notes=document.querySelector('.readerNoteList');\
                return notes?notes.innerText.trim():'';\
            })()",
            )
            .await;
        raw
    }

    /// Extract AI outline of the book (structure + key points).
    pub async fn extract_ai_outline(&self) -> String {
        let (raw, _) = self
            .kimi
            .eval_js(
                "(function(){\
                var outline=document.querySelector('.wr_outline_book_detail_main');\
                return outline?outline.innerText.trim():'';\
            })()",
            )
            .await;
        raw
    }

    // ═══════════════════════════════════════════════════════════
    // Navigation
    // ═══════════════════════════════════════════════════════════

    /// Click "下一章" (next chapter) button.
    pub async fn next_chapter(&self) -> Result<()> {
        let old_title = self.current_chapter_title().await;
        // Click the next chapter button (.readerFooter_button)
        let (ok, _) = self.kimi.eval_js(
            "(function(){var btn=document.querySelector('.readerFooter_button');if(!btn){var all=document.querySelectorAll('button');for(var i=0;i<all.length;i++){if(all[i].textContent.indexOf('下一章')>=0){btn=all[i];break}}}if(btn){btn.click();return'true'}return'false'})()"
        ).await;

        if ok != "true" {
            return Err(AdapterError::ElementNotFound {
                selector: "下一章 button".into(),
            });
        }

        // Wait for content to change
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let new_title = self.current_chapter_title().await;
            if !new_title.is_empty() && new_title != old_title {
                return Ok(());
            }
        }
        Err(AdapterError::Timeout { elapsed: 15.0 })
    }

    /// Click "上一章" (previous chapter) button.
    pub async fn prev_chapter(&self) -> Result<()> {
        let old_title = self.current_chapter_title().await;
        let (ok, _) = self.kimi.eval_js(
            "(function(){var btn=document.querySelector('.readerHeaderButton');if(!btn){var all=document.querySelectorAll('button');for(var i=0;i<all.length;i++){if(all[i].textContent.indexOf('上一章')>=0){btn=all[i];break}}};if(btn){btn.click();return'true'}return'false'})()"
        ).await;

        if ok != "true" {
            return Err(AdapterError::ElementNotFound {
                selector: "上一章 button".into(),
            });
        }

        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let new_title = self.current_chapter_title().await;
            if !new_title.is_empty() && new_title != old_title {
                return Ok(());
            }
        }
        Err(AdapterError::Timeout { elapsed: 15.0 })
    }

    // ═══════════════════════════════════════════════════════════
    // Reading progress info
    // ═══════════════════════════════════════════════════════════

    /// Get reading progress percentage from the footer.
    pub async fn reading_progress(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var el=document.querySelector('.readerFooter');if(!el)return'';var t=el.innerText;var m=t.match(/(\\d+)%/);return m?m[0]:''})()"
        ).await;
        raw.trim().to_string()
    }

    /// Get the current book's short ID from __INITIAL_STATE__.
    pub async fn current_book_id(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var s=window.__INITIAL_STATE__;return(s&&s.reader&&s.reader.bookId)?s.reader.bookId:''})()"
        ).await;
        raw.trim().to_string()
    }
}

// ═══════════════════════════════════════════════════════════════
// Internal parsers
// ═══════════════════════════════════════════════════════════════

fn parse_book_links(raw: &str) -> Vec<BookInfo> {
    let mut books = Vec::new();
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        for item in arr {
            books.push(BookInfo {
                book_id: String::new(),
                long_book_id: String::new(),
                title: item
                    .get("title")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                author: item
                    .get("author")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                cover: String::new(),
                publisher: String::new(),
                format: String::new(),
                price: 0.0,
                rating: 0,
                rating_count: 0,
                intro: String::new(),
                total_chapters: 0,
            });
        }
    }
    books
}

fn parse_book_info(raw: &str) -> Option<BookInfo> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("error").is_some() {
        return None;
    }
    Some(BookInfo {
        book_id: v
            .get("bookId")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        long_book_id: v
            .get("longBookId")
            .and_then(|s| s.as_str())
            .unwrap_or(v.get("bookId").and_then(|s| s.as_str()).unwrap_or(""))
            .to_string(),
        title: v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        author: v
            .get("author")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        cover: v
            .get("cover")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        publisher: v
            .get("publisher")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        format: v
            .get("format")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        price: v.get("price").and_then(|n| n.as_f64()).unwrap_or(0.0),
        rating: v.get("rating").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        rating_count: v.get("ratingCount").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        intro: v
            .get("intro")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        total_chapters: v.get("totalChapters").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    })
}

fn parse_chapter_list(raw: &str) -> Vec<ChapterInfo> {
    let mut chapters = Vec::new();
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        for item in arr {
            chapters.push(ChapterInfo {
                chapter_uid: item.get("chapterUid").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                chapter_idx: item.get("chapterIdx").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                title: item
                    .get("title")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                word_count: item.get("wordCount").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                level: item.get("level").and_then(|n| n.as_u64()).unwrap_or(1) as u32,
                price: item.get("price").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                paid: item.get("paid").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                tar: item
                    .get("tar")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    chapters
}
