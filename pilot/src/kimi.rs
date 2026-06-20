//! Layer 1: KimiPrimitives — generic browser operations via Kimi WebBridge HTTP API.
//!
//! Maps to the Python `KimiPrimitives` class. All interactions go through
//! POST /command with {"action", "args", "session"}.

use crate::error::{AdapterError, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:10086";
const DEFAULT_SESSION: &str = "deepseek";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Max attempts for transient (5xx / connection) errors on read-only calls.
const READONLY_RETRY_MAX: u32 = 3;
/// Backoff between read-only retries. WebBridge 502s are usually transient
/// (browser tab suspended / extension momentary unresponsiveness).
const READONLY_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Generic browser operations via Kimi WebBridge HTTP API.
#[derive(Debug, Clone)]
pub struct KimiPrimitives {
    client: Client,
    base_url: String,
    session: String,
}

impl KimiPrimitives {
    /// Create a new KimiPrimitives with custom base URL and session name.
    pub fn new(base_url: impl Into<String>, session: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            base_url: base_url.into(),
            session: session.into(),
        }
    }

    /// Returns a reference to the session name (useful for log context).
    pub fn session(&self) -> &str {
        &self.session
    }
}

impl Default for KimiPrimitives {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, DEFAULT_SESSION)
    }
}

// ── Private helpers ──

impl KimiPrimitives {
    /// Core HTTP primitive: POST to Kimi WebBridge /command.
    /// No retry — use for state-mutating actions (navigate, key_type, send_keys)
    /// where a retry could cause double-execution.
    async fn _kimi(&self, action: &str, args: Value) -> Result<Value> {
        self._kimi_once(action, args).await
    }

    /// Read-only variant with automatic retry on transient errors (5xx,
    /// connection resets). Safe for idempotent queries (evaluate, find_tab,
    /// list_tabs, get_url) — never for navigate/send.
    async fn _kimi_readonly(&self, action: &str, args: Value) -> Result<Value> {
        let mut last_err: Option<AdapterError> = None;
        for attempt in 0..READONLY_RETRY_MAX {
            match self._kimi_once(action, args.clone()).await {
                Ok(v) => return Ok(v),
                Err(e) if Self::is_transient(&e) => {
                    last_err = Some(e);
                    if attempt + 1 < READONLY_RETRY_MAX {
                        warn!(
                            action = action,
                            attempt = attempt + 1,
                            max = READONLY_RETRY_MAX,
                            error = %last_err.as_ref().unwrap(),
                            "transient WebBridge error, retrying"
                        );
                        tokio::time::sleep(READONLY_RETRY_DELAY).await;
                    }
                }
                Err(e) => return Err(e), // non-transient (business error), don't retry
            }
        }
        Err(last_err.unwrap_or_else(|| AdapterError::Kimi("retry exhausted".into())))
    }

    /// A transient error is a 5xx HTTP status or connection failure — the kind
    /// that typically resolves on its own (browser tab woke up, extension
    /// reconnected). Business errors (ok:false) are NOT transient.
    fn is_transient(err: &AdapterError) -> bool {
        match err {
            AdapterError::Http(e) => e.status().map(|s| s.is_server_error()).unwrap_or(true),
            AdapterError::Kimi(msg) => msg.contains("HTTP 5"),
            _ => false,
        }
    }

    /// Single attempt: POST to Kimi WebBridge /command.
    async fn _kimi_once(&self, action: &str, args: Value) -> Result<Value> {
        let url = format!("{}/command", self.base_url);
        let body = serde_json::json!({
            "action": action,
            "args": args,
            "session": self.session,
        });

        let start = Instant::now();
        let resp = self.client.post(&url).json(&body).send().await?;

        let elapsed = start.elapsed();
        let status = resp.status();
        let raw: Value = resp.json().await?;

        if status.is_success() {
            if let Some(false) = raw.get("ok").and_then(|v| v.as_bool()) {
                let err_msg = raw
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                return Err(AdapterError::Kimi(err_msg.to_string()));
            }
            let data = raw.get("data").cloned().unwrap_or(Value::Null);
            debug!(
                action = action,
                elapsed_ms = elapsed.as_millis() as u64,
                "kimi call ok"
            );
            Ok(data)
        } else {
            let err_text = raw
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("HTTP error");
            Err(AdapterError::Kimi(format!(
                "HTTP {}: {}",
                status.as_u16(),
                err_text
            )))
        }
    }
}

// ── Public API ──

impl KimiPrimitives {
    /// Evaluate JavaScript in the browser. Returns (value_string, exit_code).
    pub async fn eval_js(&self, script: &str) -> (String, i32) {
        trace!(script_len = script.len(), "eval_js");
        match self
            ._kimi_readonly("evaluate", serde_json::json!({"code": script}))
            .await
        {
            Ok(data) => {
                if let Some(v) = data.get("value") {
                    if v.is_null() {
                        return (String::new(), 0);
                    }
                    if v.is_boolean() {
                        return (
                            if v.as_bool().unwrap() {
                                "true"
                            } else {
                                "false"
                            }
                            .into(),
                            0,
                        );
                    }
                    // For strings, return the raw content (not JSON-encoded with quotes).
                    if let Some(s) = v.as_str() {
                        return (s.to_string(), 0);
                    }
                    // For numbers and other values, use their string representation.
                    return (v.to_string(), 0);
                }
                (String::new(), 0)
            }
            Err(e) => {
                debug!(error = %e, "eval_js failed");
                (e.to_string(), 1)
            }
        }
    }

    /// Evaluate JS that returns JSON.stringify() and parse the result.
    pub async fn eval_json(&self, script: &str) -> Option<serde_json::Value> {
        let (raw, code) = self.eval_js(script).await;
        if code != 0 {
            return None;
        }
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        // Try direct JSON parse (handles raw objects, arrays, etc.)
        if let Ok(data) = serde_json::from_str::<Value>(text) {
            if data.is_object() {
                return Some(data);
            }
            // Allow arrays too (e.g., browser log entries)
            if data.is_array() {
                return Some(data);
            }
        }
        // Try unwrapping JSON-string-literal:
        // The JS `JSON.stringify({...})` may be returned as a JSON-encoded
        // string, i.e. the raw text is `"{\"key\":\"val\"}"`.
        // We strip the outer quotes, unescape, and parse again.
        if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
            let inner = &text[1..text.len() - 1];
            // Unescape JSON escapes: \" → ", \\ → \, \n → actual newline, etc.
            let unescaped = inner
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\n", "\n")
                .replace("\\t", "\t");
            if let Ok(data) = serde_json::from_str::<Value>(&unescaped)
                && (data.is_object() || data.is_array())
            {
                return Some(data);
            }
        }
        None
    }

    /// Navigate the browser to a URL.
    pub async fn navigate(&self, url: &str, new_tab: bool) -> Result<Value> {
        self._kimi(
            "navigate",
            serde_json::json!({"url": url, "newTab": new_tab}),
        )
        .await
    }

    /// Get the current page URL via JS eval.
    pub async fn get_url(&self) -> String {
        let (val, _) = self.eval_js("window.location.href").await;
        val
    }

    /// Type text via keyboard simulation.
    pub async fn key_type(&self, text: &str) -> Result<Value> {
        self._kimi("key_type", serde_json::json!({"text": text}))
            .await
    }

    /// Send raw key combinations (e.g., "Enter", "Control+a").
    pub async fn send_keys(&self, keys: &str) -> Result<Value> {
        self._kimi("send_keys", serde_json::json!({"keys": keys}))
            .await
    }

    /// Check if a tab with URL containing the string exists.
    pub async fn find_tab(&self, url_contains: &str) -> bool {
        match self
            ._kimi_readonly("find_tab", serde_json::json!({"url": url_contains}))
            .await
        {
            Ok(data) => data
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// List all open tabs.
    pub async fn list_tabs(&self) -> Vec<TabInfo> {
        match self
            ._kimi_readonly("list_tabs", serde_json::json!({}))
            .await
        {
            Ok(data) => {
                let tabs = data.get("tabs").and_then(|v| v.as_array());
                match tabs {
                    Some(arr) => arr.iter().filter_map(TabInfo::from_json).collect(),
                    None => vec![],
                }
            }
            Err(_) => vec![],
        }
    }

    /// Close a specific tab by ID.
    pub async fn close_tab(&self, tab_id: u64) -> Result<Value> {
        self._kimi("close_tab", serde_json::json!({"tabId": tab_id}))
            .await
    }
}

/// Tab information from list_tabs.
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub tab_id: u64,
    pub url: String,
    pub title: String,
    pub active: bool,
}

impl TabInfo {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            tab_id: v.get("tabId")?.as_u64()?,
            url: v.get("url")?.as_str()?.to_string(),
            title: v
                .get("title")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            active: v.get("active").and_then(|b| b.as_bool()).unwrap_or(false),
        })
    }
}
