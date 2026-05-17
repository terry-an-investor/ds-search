use serde::{Deserialize, Serialize};

/// A single entry from the browser-side log (window.__dsLog)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserLogEntry {
    #[serde(rename = "lvl")]
    pub lvl: String,
    pub t: u64,
    pub m: String,
}
