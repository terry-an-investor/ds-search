use serde::{Deserialize, Serialize};

/// Grok model selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Fast,
}

impl Model {
    pub fn as_label(&self) -> &'static str {
        match self {
            Model::Fast => "Fast",
        }
    }
}

/// Lightweight Grok page state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GrokState {
    pub has_input: bool,
    pub has_conversation: bool,
    pub url: String,
    pub is_initialized: bool,
}
