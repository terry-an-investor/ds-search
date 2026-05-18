use serde::{Deserialize, Serialize};

/// Chat mode selection on DeepSeek
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMode {
    Quick,
    Expert,
}

impl ChatMode {
    pub fn as_label(&self) -> &'static str {
        match self {
            ChatMode::Quick => "快速模式",
            ChatMode::Expert => "专家模式",
        }
    }

    pub fn as_api_name(&self) -> &'static str {
        match self {
            ChatMode::Quick => "quick",
            ChatMode::Expert => "expert",
        }
    }
}

/// Toggleable features
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Thinking,
    Search,
}

impl Feature {
    pub fn as_label(&self) -> &'static str {
        match self {
            Feature::Thinking => "深度思考",
            Feature::Search => "智能搜索",
        }
    }
}

/// Lightweight page state snapshot
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FastState {
    #[serde(default)]
    pub has_input: bool,
    #[serde(default)]
    pub is_streaming: bool,
    #[serde(default)]
    pub message_count: usize,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub has_conversation: bool,
    #[serde(default)]
    pub title: String,
}

/// A single chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// A conversation turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub user_message: String,
    pub assistant_response: String,
    pub thinking_trace: Option<ThinkingTrace>,
    pub timestamp: f64,
}

/// Thinking content from DeepSeek
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingTrace {
    pub content: String,
    pub time: Option<String>,
}

/// Full page state returned by observer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageState {
    pub url: String,
    pub is_initial_page: bool,
    pub has_input: bool,
    pub is_streaming: bool,
    pub message_count: usize,
    pub mode: String,
    pub deep_thinking_enabled: bool,
    pub web_search_enabled: bool,
}

impl Default for PageState {
    fn default() -> Self {
        Self {
            url: String::new(),
            is_initial_page: false,
            has_input: false,
            is_streaming: false,
            message_count: 0,
            mode: "unknown".into(),
            deep_thinking_enabled: false,
            web_search_enabled: false,
        }
    }
}

/// Struct returned by wait_for_response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseResult {
    pub success: bool,
    pub response: String,
    pub thinking: Option<ThinkingTrace>,
}

/// A single entry from the browser-side log (window.__dsLog)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserLogEntry {
    #[serde(rename = "lvl")]
    pub lvl: String,
    pub t: u64,
    pub m: String,
}
