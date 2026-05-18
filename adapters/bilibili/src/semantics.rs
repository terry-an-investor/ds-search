//! BilibiliSemantics — Bilibili video platform operations via JS eval.
//!
//! Non-AI-chat adapter. Operations:
//! - search(keyword) → navigate to search results
//! - extract_results() → get video list from search page
//! - go_to_page(n) → navigate to page N
//! - sort_by(order) → click sort button
//! - extract_video_details() → get info from video page

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::{SortOrder, VideoDetails, VideoResult};

const BILIBILI_URL: &str = "https://www.bilibili.com";
const SEARCH_URL: &str = "https://search.bilibili.com/all";

#[derive(Debug, Clone)]
pub struct BilibiliSemantics {
    pub kimi: KimiPrimitives,
}

impl BilibiliSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    /// Ensure we're on a bilibili page.
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("bilibili.com") {
            self.kimi.navigate(BILIBILI_URL, false).await?;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Ok(())
    }

    /// Search for videos by keyword. Navigates to the search results page.
    pub async fn search(&self, keyword: &str) -> Result<()> {
        let search_url = format!("{}?keyword={}", SEARCH_URL, keyword);
        self.kimi.navigate(&search_url, false).await?;
        // Wait for results to load
        for _ in 0..30 {
            let (count_str, _) = self.kimi.eval_js(
                "document.querySelectorAll('.video.i_wrapper .bili-video-card').length"
            ).await;
            let count: usize = count_str.parse().unwrap_or(0);
            if count > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Navigate to a specific page of search results.
    pub async fn go_to_page(&self, page: u32) -> Result<()> {
        let url = self.kimi.get_url().await;
        let base = if url.contains("?") {
            url.split('?').next().unwrap_or(SEARCH_URL)
        } else {
            SEARCH_URL
        };
        // Extract keyword from current URL
        let keyword = extract_query_param(&url, "keyword").unwrap_or_default();
        let page_url = format!("{}?keyword={}&page={}", base, keyword, page);
        self.kimi.navigate(&page_url, false).await?;
        for _ in 0..30 {
            let (count_str, _) = self.kimi.eval_js(
                "document.querySelectorAll('.video.i_wrapper .bili-video-card').length"
            ).await;
            let count: usize = count_str.parse().unwrap_or(0);
            if count > 0 { break; }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Extract video results from the current search page.
    pub async fn extract_results(&self, max: usize) -> Vec<VideoResult> {
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify(Array.from(document.querySelectorAll('.video.i_wrapper .bili-video-card')).slice(0,{}).map(function(e){{
                const titleEl=e.querySelector('[title],.bili-video-card__info--tit');
                const a=e.querySelector('a[href*="/video/"]');
                const durEl=e.querySelector('[class*=duration]');
                const upEl=e.querySelector('[class*=author]');
                const dateEl=e.querySelector('[class*=date]');
                const isAd=!!e.querySelector('a[href*="cm.bilibili.com"]');
                return {{
                    title:titleEl?(titleEl.getAttribute('title')||titleEl.textContent||'').trim().replace(/\\s+/g,' '):'',
                    url:a?a.href:'',
                    duration:durEl?durEl.textContent.trim():'',
                    views:'',
                    uploader:upEl?upEl.textContent.trim():'',
                    upload_date:dateEl?dateEl.textContent.trim():'',
                    is_ad:isAd
                }};
            }}))"#,
            max
        )).await;
        parse_video_results(&raw)
    }

    /// Sort search results.
    pub async fn sort_by(&self, order: SortOrder) -> Result<()> {
        let label = order.as_label();
        let (found, _) = self.kimi.eval_js(&format!(
            r#"(function(){{
                const btns=document.querySelectorAll('button');
                for(let i=0;i<btns.length;i++){{
                    if((btns[i].textContent||'').includes('{}')){{
                        btns[i].click();return'true';
                    }}
                }}
                return'false';
            }})()"#,
            label
        )).await;
        if found != "true" {
            return Err(AdapterError::ElementNotFound {
                selector: format!("sort button '{}'", label),
            });
        }
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Navigate to a specific video page and extract details.
    pub async fn extract_video_details(&self, video_url: Option<&str>) -> Result<VideoDetails> {
        if let Some(url) = video_url {
            self.kimi.navigate(url, false).await?;
        }
        // Wait for page load
        for _ in 0..30 {
            let (title_str, _) = self.kimi.eval_js("document.title").await;
            if !title_str.is_empty() && title_str != "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let (raw, _) = self.kimi.eval_js(
            r#"JSON.stringify((()=>{
                const title=document.title.replace('_哔哩哔哩_bilibili','').trim();
                const url=location.href.split('?')[0];
                const body=document.body?document.body.innerText:'';
                const lines=body.split('\n').filter(function(l){return l.trim().length>0});
                const tags=Array.from(document.querySelectorAll('.tag-link,.video-tag')).map(function(t){return t.textContent.trim()});
                const descEl=document.querySelector('.video-desc,.basic-desc-info,.desc-info-text');
                const description=descEl?descEl.textContent.trim().substring(0,2000):'';
                return {
                    title:title, url:url,
                    body_first_lines:lines.slice(0,30),
                    tags:tags,
                    description:description
                };
            })())"#,
        ).await;

        let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        let mut details = VideoDetails {
            title: v.get("title").and_then(|s|s.as_str()).unwrap_or("").to_string(),
            url: v.get("url").and_then(|s|s.as_str()).unwrap_or("").to_string(),
            description: v.get("description").and_then(|s|s.as_str()).unwrap_or("").to_string(),
            tags: v.get("tags").and_then(|a|a.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            ..Default::default()
        };

        // Extract stats from body lines
        if let Some(lines) = v.get("body_first_lines").and_then(|a|a.as_array()) {
            let texts: Vec<&str> = lines.iter().filter_map(|l| l.as_str()).collect();
            details = extract_stats_from_lines(details, &texts);
        }

        Ok(details)
    }

    /// Get current page number from URL.
    pub async fn current_page(&self) -> u32 {
        let url = self.kimi.get_url().await;
        extract_query_param(&url, "page")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
    }
}

// ── Helpers ──

fn extract_query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            return parts.next();
        }
    }
    None
}

fn parse_video_results(raw: &str) -> Vec<VideoResult> {
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .map(|item| VideoResult {
            title: item.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            url: item.get("url").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            duration: item.get("duration").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            views: item.get("views").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            uploader: item.get("uploader").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            upload_date: item.get("upload_date").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            is_ad: item.get("is_ad").and_then(|b| b.as_bool()).unwrap_or(false),
        })
        .collect()
}

fn extract_stats_from_lines(mut details: VideoDetails, lines: &[&str]) -> VideoDetails {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.contains("万") && details.views.is_empty()
            && !trimmed.contains(":") && !trimmed.contains("-")
        {
            details.views = trimmed.to_string();
        } else if (trimmed.contains("赞") || trimmed.contains("点赞"))
            && details.likes.is_empty()
        {
            details.likes = trimmed.to_string();
        } else if trimmed.contains("币") && details.coins.is_empty() {
            details.coins = trimmed.to_string();
        } else if trimmed.contains("藏") && details.favorites.is_empty() {
            details.favorites = trimmed.to_string();
        } else if trimmed.contains("20") && trimmed.contains("-") && details.upload_date.is_empty() {
            details.upload_date = trimmed.to_string();
        }
        // Uploader: line before 发消息
        if trimmed == "发消息" && details.uploader.is_empty() {
            // Find previous non-empty line
            for j in 0..lines.len() {
                if lines[j] == "发消息" && j > 0 {
                    details.uploader = lines[j - 1].trim().to_string();
                    break;
                }
            }
        }
    }
    details
}
