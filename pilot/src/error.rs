use thiserror::Error;

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Kimi WebBridge returned error: {0}")]
    Kimi(String),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Timeout waiting for response after {elapsed}s")]
    Timeout { elapsed: f64 },

    #[error("Page not ready: {reason}")]
    PageNotReady { reason: String },

    #[error("DOM element not found: {selector}")]
    ElementNotFound { selector: String },

    #[error("Send message failed: {reason}")]
    SendFailed { reason: String },

    #[error("No response extracted")]
    NoResponse,
}

impl From<&str> for AdapterError {
    fn from(s: &str) -> Self {
        AdapterError::Kimi(s.to_string())
    }
}

impl From<String> for AdapterError {
    fn from(s: String) -> Self {
        AdapterError::Kimi(s)
    }
}

pub type Result<T> = std::result::Result<T, AdapterError>;
