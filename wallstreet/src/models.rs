//! Data models for WallstreetCN.

/// A single news flash item from the live feed.
#[derive(Debug, Clone)]
pub struct LiveItem {
    pub time: String,      // e.g. "21:17"
    pub title: String,     // e.g. "消息人士：以色列在伊拉克..."
    pub content: String,   // full body text after the title
    pub index: usize,      // position in feed (0 = newest)
}

/// An article from the homepage.
#[derive(Debug, Clone)]
pub struct Article {
    pub title: String,
    pub url: String,
    pub summary: String,
}

/// Category tabs on the live feed page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCategory {
    Global,    // 要闻 (default)
    AStock,    // A股
    USStock,   // 美股
    HKStock,   // 港股
    Forex,     // 外汇
    Commodity, // 商品
    Bond,      // 债券
    Tech,      // 科技
}

impl LiveCategory {
    pub fn as_label(&self) -> &'static str {
        match self {
            LiveCategory::Global => "要闻",
            LiveCategory::AStock => "A股",
            LiveCategory::USStock => "美股",
            LiveCategory::HKStock => "港股",
            LiveCategory::Forex => "外汇",
            LiveCategory::Commodity => "商品",
            LiveCategory::Bond => "债券",
            LiveCategory::Tech => "科技",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "要闻" => Some(LiveCategory::Global),
            "A股" => Some(LiveCategory::AStock),
            "美股" => Some(LiveCategory::USStock),
            "港股" => Some(LiveCategory::HKStock),
            "外汇" => Some(LiveCategory::Forex),
            "商品" => Some(LiveCategory::Commodity),
            "债券" => Some(LiveCategory::Bond),
            "科技" => Some(LiveCategory::Tech),
            _ => None,
        }
    }
}
