//! Data models for Bilibili video platform.

/// A video search result from bilibili.
#[derive(Debug, Clone)]
pub struct VideoResult {
    pub title: String,
    pub url: String,
    pub duration: String,
    pub views: String,
    pub uploader: String,
    pub upload_date: String,
    pub is_ad: bool,
}

/// Sort order for search results.
#[derive(Debug, Clone, Copy)]
pub enum SortOrder {
    Relevance,     // 综合排序
    MostPlayed,    // 最多播放
    Newest,        // 最新发布
    MostDanmaku,   // 最多弹幕
    MostFavorited, // 最多收藏
}

impl SortOrder {
    pub fn as_label(&self) -> &'static str {
        match self {
            SortOrder::Relevance => "综合排序",
            SortOrder::MostPlayed => "最多播放",
            SortOrder::Newest => "最新发布",
            SortOrder::MostDanmaku => "最多弹幕",
            SortOrder::MostFavorited => "最多收藏",
        }
    }
}

/// Video page details.
#[derive(Debug, Clone, Default)]
pub struct VideoDetails {
    pub title: String,
    pub url: String,
    pub views: String,
    pub likes: String,
    pub coins: String,
    pub favorites: String,
    pub upload_date: String,
    pub uploader: String,
    pub description: String,
    pub tags: Vec<String>,
}
