//! Data models for X.com tweets.

use serde::{Deserialize, Serialize};

/// A tweet from a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tweet {
    pub author: String,
    pub text: String,
    pub timestamp: String,
    pub url: String,
    pub external_links: Vec<ExternalLink>,
    pub stats: TweetStats,
    pub is_main_tweet: bool,
}

/// External link found in a tweet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLink {
    pub url: String,
    pub display_url: String,
    pub title: String,
}

/// Tweet engagement stats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TweetStats {
    pub replies: u32,
    pub reposts: u32,
    pub likes: u32,
    pub views: u32,
}

/// A complete thread (main tweet + self-replies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub main_tweet: Tweet,
    pub replies: Vec<Tweet>,
    pub total_tweets: usize,
}
