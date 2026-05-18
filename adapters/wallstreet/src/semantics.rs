//! WallstreetSemantics — WallstreetCN financial news operations.
//!
//! Two adapters:
//! - WallstreetSemantics: homepage, search, articles
//! - LiveGlobalSemantics: real-time newsfeed at /live/global

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::{Article, LiveCategory, LiveItem};

const WALLSTREET_URL: &str = "https://wallstreetcn.com";
const LIVE_GLOBAL_URL: &str = "https://wallstreetcn.com/live/global";

// ═══════════════════════════════════════════════════════════════
// General adapter (homepage, search, articles)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct WallstreetSemantics {
    pub kimi: KimiPrimitives,
}

impl WallstreetSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("wallstreetcn.com") {
            self.kimi.navigate(WALLSTREET_URL, false).await?;
        }
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Extract article links from the current page.
    pub async fn extract_articles(&self, max: usize) -> Vec<Article> {
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify(Array.from(document.querySelectorAll('a[href*="/articles/"]')).slice(0,{}).map(function(a){{
                const titleEl=a.querySelector('h3,h2,[class*=title]');
                return {{
                    title:(titleEl?titleEl.textContent:a.textContent||'').trim().replace(/\\s+/g,' ').substring(0,120),
                    url:a.getAttribute('href'),
                    summary:''
                }};
            }}))"#,
            max
        )).await;
        parse_articles(&raw)
    }

    /// Search for articles by keyword (site search).
    pub async fn search(&self, keyword: &str) -> Result<()> {
        // Focus search input, type, and submit
        let _ = self.kimi.eval_js(
            "(function(){var i=document.querySelector('input[placeholder=搜索]');if(i){i.focus();i.value=''}})()"
        ).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.kimi.key_type(keyword).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.kimi.send_keys("Enter").await?;
        // Wait for results
        for _ in 0..20 {
            let (count, _) = self.kimi.eval_js(
                "document.querySelectorAll('a[href*=\"/articles/\"]').length"
            ).await;
            let n: usize = count.parse().unwrap_or(0);
            if n > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Navigate to a specific article and extract its body text.
    pub async fn extract_article_body(&self, url: Option<&str>) -> Result<String> {
        if let Some(u) = url {
            self.kimi.navigate(u, false).await?;
            tokio::time::sleep(Duration::from_millis(2000)).await;
        }
        let (body, _) = self.kimi.eval_js(
            r#"(function(){
                const sel='.article-body,.article-content,.rich-text,.article';
                const el=document.querySelector(sel);
                return el?el.textContent.trim().substring(0,5000):document.body?.innerText?.substring(0,3000)||'';
            })()"#,
        ).await;
        Ok(body)
    }
}

// ═══════════════════════════════════════════════════════════════
// Live/Global adapter (real-time newsfeed)
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct LiveGlobalSemantics {
    pub kimi: KimiPrimitives,
}

impl LiveGlobalSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    /// Navigate to the live feed page.
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("wallstreetcn.com/live") {
            self.kimi.navigate(LIVE_GLOBAL_URL, false).await?;
        }
        // Wait for live items to appear
        for _ in 0..30 {
            let (count_str, _) = self.kimi.eval_js(
                "document.querySelectorAll('.live-item').length"
            ).await;
            let count: usize = count_str.parse().unwrap_or(0);
            if count > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Switch to a different category tab.
    pub async fn switch_category(&self, category: LiveCategory) -> Result<()> {
        let label = category.as_label();
        let (found, _) = self.kimi.eval_js(&format!(
            r#"(function(){{
                const items=document.querySelectorAll('.nav-item');
                for(let i=0;i<items.length;i++){{
                    if(items[i].textContent.trim()==='{}'){{
                        items[i].click();return'true';
                    }}
                }}
                return'false';
            }})()"#,
            label
        )).await;
        if found != "true" {
            return Err(AdapterError::ElementNotFound {
                selector: format!("category tab '{}'", label),
            });
        }
        // Wait for items to reload
        for _ in 0..30 {
            let (count_str, _) = self.kimi.eval_js(
                "document.querySelectorAll('.live-item').length"
            ).await;
            let count: usize = count_str.parse().unwrap_or(0);
            if count > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Get the current active category.
    pub async fn current_category(&self) -> Option<LiveCategory> {
        let (label, _) = self.kimi.eval_js(
            "(function(){var el=document.querySelector('.nav-item.active');return el?el.textContent.trim():''})()"
        ).await;
        LiveCategory::from_label(&label)
    }

    /// Extract live feed items from the current page.
    /// Each item has format: "HH:MM 【title】content..."
    pub async fn extract_items(&self, max: usize) -> Vec<LiveItem> {
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify(Array.from(document.querySelectorAll('.live-item')).slice(0,{}).map(function(e,i){{
                const text=e.textContent.trim().replace(/\\s+/g,' ');
                const timeMatch=text.match(/^(\\d{{2}}:\\d{{2}})\\s/);
                const titleMatch=text.match(/【(.+?)】/);
                let content=text;
                if(timeMatch)content=content.substring(timeMatch[0].length);
                if(titleMatch)content=content.replace('【'+titleMatch[1]+'】','').trim();
                return {{
                    time:timeMatch?timeMatch[1]:'',
                    title:titleMatch?titleMatch[1]:'',
                    content:content.substring(0,500),
                    index:i
                }};
            }}))"#,
            max
        )).await;
        parse_live_items(&raw)
    }

    /// Toggle the "只看重要的" (important only) filter.
    pub async fn toggle_important_only(&self) -> Result<bool> {
        let (toggled, _) = self.kimi.eval_js(
            r#"(function(){
                const els=document.querySelectorAll('.live-settings [class*=btn],.live-settings span,.live-settings div');
                for(let i=0;i<els.length;i++){
                    if((els[i].textContent||'').includes('只看重要的')){
                        els[i].click();return els[i].classList.contains('active')?'active':'clicked';
                    }
                }
                // Also try the label text
                const labels=document.querySelectorAll('.live-settings *');
                for(let i=0;i<labels.length;i++){
                    if((labels[i].textContent||'').trim()==='只看重要的'){
                        labels[i].click();return'clicked_label';
                    }
                }
                return'not found';
            })()"#,
        ).await;
        if toggled.contains("not found") {
            return Err(AdapterError::ElementNotFound {
                selector: "只看重要的 toggle".into(),
            });
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
        Ok(toggled.contains("active"))
    }

    /// Get the page header time string (e.g. "05月17日， 星期日， 21:18:51").
    pub async fn header_time(&self) -> String {
        let (time, _) = self.kimi.eval_js(
            r#"(function(){var el=document.querySelector('.livenews-main');if(!el)return'';var t=el.textContent.match(/\d{2}月\d{2}日[,，]\s*\S+[,，]\s*\d{2}:\d{2}:\d{2}/);return t?t[0]:''})()"#
        ).await;
        time
    }

    /// Poll for new items since last check. Returns newly appeared items.
    pub async fn poll_new_items(&self, last_count: usize) -> Vec<LiveItem> {
        let (count_str, _) = self.kimi.eval_js(
            "document.querySelectorAll('.live-item').length"
        ).await;
        let count: usize = count_str.parse().unwrap_or(0);
        if count <= last_count {
            return vec![];
        }
        let new_count = count - last_count;
        self.extract_items(new_count).await
    }
}

// ── Helpers ──

fn parse_articles(raw: &str) -> Vec<Article> {
    let v: serde_json::Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => return vec![] };
    let arr = match v.as_array() { Some(a) => a, None => return vec![] };
    arr.iter().map(|item| Article {
        title: item.get("title").and_then(|s|s.as_str()).unwrap_or("").to_string(),
        url: item.get("url").and_then(|s|s.as_str()).unwrap_or("").to_string(),
        summary: item.get("summary").and_then(|s|s.as_str()).unwrap_or("").to_string(),
    }).collect()
}

fn parse_live_items(raw: &str) -> Vec<LiveItem> {
    let v: serde_json::Value = match serde_json::from_str(raw) { Ok(v) => v, Err(_) => return vec![] };
    let arr = match v.as_array() { Some(a) => a, None => return vec![] };
    arr.iter().map(|item| LiveItem {
        time: item.get("time").and_then(|s|s.as_str()).unwrap_or("").to_string(),
        title: item.get("title").and_then(|s|s.as_str()).unwrap_or("").to_string(),
        content: item.get("content").and_then(|s|s.as_str()).unwrap_or("").to_string(),
        index: item.get("index").and_then(|n|n.as_u64()).unwrap_or(0) as usize,
    }).collect()
}
