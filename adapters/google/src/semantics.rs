//! GoogleSemantics — Google Search operations.
//!
//! Features: search, extract results, featured snippets, pagination, time filters.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::SearchResult;

const GOOGLE_URL: &str = "https://www.google.com";
const SEARCH_BASE: &str = "https://www.google.com/search";

#[derive(Debug, Clone)]
pub struct GoogleSemantics {
    pub kimi: KimiPrimitives,
}

impl GoogleSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ═══════════════════════════════════════════════════════════
    // Tab management
    // ═══════════════════════════════════════════════════════════

    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("google.com") {
            self.kimi.navigate(GOOGLE_URL, false).await?;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Ok(())
    }

    /// Navigate to the homepage.
    pub async fn go_home(&self) -> Result<()> {
        self.kimi.navigate(GOOGLE_URL, false).await?;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Search
    // ═══════════════════════════════════════════════════════════

    /// Search by navigating directly to search URL (most reliable).
    pub async fn search(&self, query: &str) -> Result<()> {
        self.search_url(query, "").await
    }

    /// Search with a search mode (tab).
    /// mode: "ai", "images", "videos", "shopping", "short_videos",
    ///       "forums", "news", "web", "books", "all" (default)
    pub async fn search_mode(&self, query: &str, mode: &str) -> Result<()> {
        let param = match mode {
            "ai" => "udm=50",
            "images" => "udm=2",
            "videos" => "udm=7",
            "shopping" => "udm=28",
            "short_videos" => "udm=39",
            "forums" => "udm=18",
            "news" => "tbm=nws",
            "web" => "udm=web",
            "books" => "udm=36",
            "all" | "" => "",
            _ => return Err(AdapterError::Kimi(format!("unknown mode '{}'. Use: all,ai,images,videos,shopping,short_videos,forums,news,web,books", mode))),
        };
        self.search_url(query, param).await
    }

    /// Internal: navigate to search URL with optional extra param.
    async fn search_url(&self, query: &str, extra_param: &str) -> Result<()> {
        let encoded = urlencoding(query);
        let url = if extra_param.is_empty() {
            format!("{}?q={}", SEARCH_BASE, encoded)
        } else {
            format!("{}?q={}&{}", SEARCH_BASE, encoded, extra_param)
        };
        self.kimi.navigate(&url, false).await?;

        for _ in 0..20 {
            let (count, _) = self.kimi.eval_js(
                "document.querySelectorAll('h3').length"
            ).await;
            let n: usize = count.parse().unwrap_or(0);
            if n > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Search with a time filter.
    /// period: "h"=hour, "d"=day, "w"=week, "m"=month, "y"=year
    pub async fn search_with_time(&self, query: &str, period: &str) -> Result<()> {
        let encoded = urlencoding(query);
        let url = format!("{}?q={}&tbs=qdr:{}", SEARCH_BASE, encoded, period);
        self.kimi.navigate(&url, false).await?;

        for _ in 0..20 {
            let (count, _) = self.kimi.eval_js(
                "document.querySelectorAll('h3').length"
            ).await;
            let n: usize = count.parse().unwrap_or(0);
            if n > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Search on the homepage by typing into the search box.
    pub async fn search_homepage(&self, query: &str) -> Result<()> {
        self.go_home().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Type into search textarea
        let _ = self.kimi.eval_js(&format!(
            "(function(){{var el=document.querySelector('textarea[name=q]');if(el){{el.focus();el.value='{}';el.dispatchEvent(new Event('input',{{bubbles:true}}))}}}})()",
            escape_js(query)
        )).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Submit the form
        let _ = self.kimi.eval_js(
            "(function(){var el=document.querySelector('textarea[name=q]');if(el&&el.form){el.form.submit()}})()"
        ).await;

        // Wait for results
        for _ in 0..20 {
            let (count, _) = self.kimi.eval_js(
                "document.querySelectorAll('h3').length"
            ).await;
            let n: usize = count.parse().unwrap_or(0);
            if n > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Results extraction
    // ═══════════════════════════════════════════════════════════

    /// Extract search results from the current page.
    /// Works across all search modes (All, Images, News, Videos, etc.)
    pub async fn extract_results(&self, max: usize) -> Vec<SearchResult> {
        // Primary: h3-based extraction (All, Images, Videos, Shopping, etc.)
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify(Array.from(document.querySelectorAll('h3')).slice(0,{}).map(function(h3){{
                var link=h3.closest('a')||h3.querySelector('a');
                var href=link?link.href:'';
                var container=h3.closest('[data-sokoban-container],.g,[class*=MjjYud],[class*=Ww4FFb]')||h3.parentElement.parentElement;
                var snippet=container?container.innerText.substring(0,300).replace(/\\s+/g,' '):'';
                return {{
                    title:h3.textContent.trim().substring(0,200),
                    url:href,
                    snippet:snippet
                }};
            }}))"#,
            max
        )).await;
        let results = parse_results(&raw);

        // Fallback: if no h3 results, extract from news/article cards
        if results.is_empty() {
            let (raw2, _) = self.kimi.eval_js(&format!(
                r#"JSON.stringify(Array.from(document.querySelectorAll('a[href]')).filter(function(a){{
                    var h=a.href||'';return h.indexOf('google.com')<0&&h.indexOf('javascript')<0&&h.startsWith('http')&&a.textContent.trim().length>20
                }}).slice(0,{}).map(function(a){{
                    return {{
                        title:a.textContent.trim().substring(0,200),
                        url:a.href||'',
                        snippet:''
                    }};
                }}))"#,
                max
            )).await;
            return parse_results(&raw2);
        }
        results
    }

    /// Get the search stats line (e.g. "About 39,600,000 results (0.25s)").
    pub async fn result_stats(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var el=document.querySelector('#result-stats');return el?el.innerText.trim():''})()"
        ).await;
        raw.trim().to_string()
    }

    /// Check if there's a next page available.
    pub async fn has_next_page(&self) -> bool {
        let (raw, _) = self.kimi.eval_js(
            "!!document.querySelector('#pnnext')"
        ).await;
        raw == "true"
    }

    /// Navigate to the next page of results.
    pub async fn next_page(&self) -> Result<()> {
        let (ok, _) = self.kimi.eval_js(
            "(function(){var nx=document.querySelector('#pnnext');if(nx){nx.click();return'true'}return'false'})()"
        ).await;

        if ok != "true" {
            return Err(AdapterError::ElementNotFound {
                selector: "next page button".into(),
            });
        }

        // Wait for new results
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let (count, _) = self.kimi.eval_js(
                "document.querySelectorAll('h3').length"
            ).await;
            if count.parse::<usize>().unwrap_or(0) > 0 { break; }
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Featured snippets / knowledge panels
    // ═══════════════════════════════════════════════════════════

    /// Extract featured snippet or knowledge panel text.
    pub async fn extract_featured_snippet(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){\
                var el=document.querySelector('.ifM9O,.EyBRub,.kp-wholepage,.LMRCxd');\
                if(!el)return'';\
                var t=el.innerText.trim();\
                if(t.indexOf('Choose what')>=0||t.indexOf('feedback')>=0)return'';\
                return t;\
            })()"
        ).await;
        raw.trim().to_string()
    }

    /// Extract AI Overview (Google's AI-generated summary) if present.
    pub async fn extract_ai_overview(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){\
                var el=document.querySelector('[class*=ai],.hdVBI');\
                return el?el.innerText.trim():'';\
            })()"
        ).await;
        raw.trim().to_string()
    }

    // ═══════════════════════════════════════════════════════════
    // AI Mode
    // ═══════════════════════════════════════════════════════════

    /// Check if AI Mode is available and toggle it.
    pub async fn toggle_ai_mode(&self) -> Result<()> {
        let (ok, _) = self.kimi.eval_js(
            "(function(){var link=document.querySelector('a[href*=udm=50],[aria-label*=AI]');if(link){window.location.href=link.href;return'true'}return'false'})()"
        ).await;

        if ok != "true" {
            return Err(AdapterError::Kimi("AI Mode not available".into()));
        }

        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Current query
    // ═══════════════════════════════════════════════════════════

    /// Get the current search query from the page.
    pub async fn current_query(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            "(function(){var el=document.querySelector('textarea[name=q],input[name=q]');return el?el.value.trim():''})()"
        ).await;
        raw.trim().to_string()
    }

    /// Get the current page URL.
    pub async fn current_url(&self) -> String {
        self.kimi.get_url().await
    }
}

// ═══════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════

fn parse_results(raw: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        for item in arr {
            results.push(SearchResult {
                title: item.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                url: item.get("url").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                snippet: item.get("snippet").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    results
}

/// Simple URL encoding for search queries.
fn urlencoding(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' |
            b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(hex_char(byte >> 4));
                encoded.push(hex_char(byte & 0xF));
            }
        }
    }
    encoded
}

fn hex_char(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
}

/// Escape a string for safe embedding in JavaScript single-quoted string.
fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
