//! XSemantics — X.com tweet thread operations via JS eval.
//!
//! Operations:
//! - navigate_to_tweet(url) → load tweet page
//! - extract_thread() → get main tweet + self-replies
//! - scroll_to_load_replies() → trigger lazy loading of thread
//! - extract_external_links() → get all non-X links from thread

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::{ExternalLink, Thread, Tweet};

/// Reserved for future ensure_tab() implementation.
#[allow(dead_code)]
const X_URL: &str = "https://x.com";

#[derive(Debug, Clone)]
pub struct XSemantics {
    pub kimi: KimiPrimitives,
}

impl XSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    /// Navigate to a tweet URL.
    pub async fn navigate_to_tweet(&self, url: &str) -> Result<()> {
        self.kimi.navigate(url, false).await?;
        // Wait for page to load
        for _ in 0..30 {
            let (count_str, _) = self
                .kimi
                .eval_js("document.querySelectorAll('article').length")
                .await;
            let count: usize = count_str.parse().unwrap_or(0);
            if count > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Scroll down to trigger lazy loading of thread replies.
    pub async fn scroll_to_load_replies(&self, scroll_count: u32) -> Result<()> {
        for _ in 0..scroll_count {
            self.kimi.eval_js("window.scrollBy(0, 500)").await;
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
        Ok(())
    }

    /// Extract the complete thread (main tweet + self-replies by same author).
    pub async fn extract_thread(&self) -> Result<Thread> {
        let (raw, _) = self.kimi.eval_js(
            r#"(function(){
                var articles = document.querySelectorAll('article');
                if (articles.length === 0) return JSON.stringify({error: 'no articles found'});
                
                var results = [];
                for (var i = 0; i < articles.length; i++) {
                    var a = articles[i];
                    
                    // Get author from first link
                    var authorLinks = a.querySelectorAll('a[href*="/"]');
                    var author = '';
                    for (var j = 0; j < authorLinks.length; j++) {
                        var href = authorLinks[j].getAttribute('href') || '';
                        if (href.startsWith('/') && !href.includes('/status/')) {
                            author = href.substring(1);
                            break;
                        }
                    }
                    
                    // Get tweet text
                    var textEl = a.querySelector('[data-testid="tweetText"]');
                    var text = textEl ? textEl.innerText : '';
                    
                    // Get timestamp
                    var timeEl = a.querySelector('time');
                    var timestamp = timeEl ? timeEl.getAttribute('datetime') || '' : '';
                    
                    // Get tweet URL
                    var tweetLinks = a.querySelectorAll('a[href*="/status/"]');
                    var url = '';
                    for (var k = 0; k < tweetLinks.length; k++) {
                        var href = tweetLinks[k].getAttribute('href') || '';
                        if (href.includes('/status/')) {
                            url = 'https://x.com' + href;
                            break;
                        }
                    }
                    
                    // Get external links (not x.com or twitter.com)
                    var allLinks = a.querySelectorAll('a[href]');
                    var externalLinks = [];
                    for (var l = 0; l < allLinks.length; l++) {
                        var href = allLinks[l].getAttribute('href') || '';
                        if (!href.includes('x.com') && !href.includes('twitter.com') && href.startsWith('http')) {
                            externalLinks.push({
                                url: href,
                                display_url: href,
                                title: ''
                            });
                        }
                    }
                    
                    // Get stats from aria-labels
                    var stats = {replies: 0, reposts: 0, likes: 0, views: 0};
                    var buttons = a.querySelectorAll('button[aria-label]');
                    for (var m = 0; m < buttons.length; m++) {
                        var label = buttons[m].getAttribute('aria-label') || '';
                        var match = label.match(/(\d+)/);
                        if (match) {
                            var num = parseInt(match[1]);
                            if (label.includes('Repl')) stats.replies = num;
                            else if (label.includes('repost') || label.includes('Repost')) stats.reposts = num;
                            else if (label.includes('Like') || label.includes('like')) stats.likes = num;
                        }
                    }
                    // Views from analytics link
                    var analyticsLinks = a.querySelectorAll('a[href*="/analytics"]');
                    for (var n = 0; n < analyticsLinks.length; n++) {
                        var viewText = analyticsLinks[n].innerText || '';
                        var viewMatch = viewText.match(/(\d+)/);
                        if (viewMatch) stats.views = parseInt(viewMatch[1]);
                    }
                    
                    results.push({
                        author: author,
                        text: text,
                        timestamp: timestamp,
                        url: url,
                        external_links: externalLinks,
                        stats: stats,
                        is_main_tweet: i === 0
                    });
                }
                return JSON.stringify(results);
            })()"#
        ).await;

        let tweets: Vec<Tweet> = serde_json::from_str(&raw).unwrap_or_default();

        if tweets.is_empty() {
            return Err(AdapterError::ElementNotFound {
                selector: "article".to_string(),
            });
        }

        let main_tweet = tweets[0].clone();
        let replies = tweets[1..].to_vec();
        let total_tweets = tweets.len();

        Ok(Thread {
            main_tweet,
            replies,
            total_tweets,
        })
    }

    /// Extract only external links from all tweets in the thread.
    pub async fn extract_external_links(&self) -> Vec<ExternalLink> {
        let thread = match self.extract_thread().await {
            Ok(t) => t,
            Err(_) => return vec![],
        };

        let mut links = Vec::new();
        links.extend(thread.main_tweet.external_links);
        for reply in thread.replies {
            links.extend(reply.external_links);
        }
        links
    }

    /// Get the username from the current tweet URL.
    pub async fn get_username_from_url(&self) -> String {
        let url = self.kimi.get_url().await;
        // Extract username from https://x.com/username/status/...
        if let Some(path) = url.strip_prefix("https://x.com/")
            && let Some(username) = path.split('/').next()
        {
            return username.to_string();
        }
        String::new()
    }
}
