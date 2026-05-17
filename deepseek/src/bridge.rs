//! Layer 3: DeepSeekAgentBridge — combined observer + actor pipeline.
//!
//! Maps to the Python `DeepSeekAgentBridge` class. Orchestrates the full
//! send pipeline: ensure tab → wait ready → send → wait response → extract.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use crate::models::{Feature, ChatMode};
use crate::semantics::DeepSeekSemantics;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// Polling constants (match Python values)
const POLL_INITIAL: f64 = 0.1;
const POLL_BACKOFF: f64 = 1.3;
const POLL_CAP: f64 = 0.5;
const TIMEOUT_READY: f64 = 15.0;
const TIMEOUT_STREAM_FINISH: f64 = 60.0;
const EXTRACT_RETRIES: usize = 20;
const COOLDOWN_SECS: f64 = 0.5; // content must be unchanged for this long

/// Combined observer + actor for DeepSeek chat.
#[derive(Clone)]
pub struct DeepSeekAgentBridge {
    pub kimi: KimiPrimitives,
    pub semantics: DeepSeekSemantics,
    lock: Arc<Mutex<()>>,
}

impl DeepSeekAgentBridge {
    /// Create a new bridge with the given primitives.
    pub fn new(kimi: KimiPrimitives) -> Self {
        let semantics = DeepSeekSemantics::new(kimi.clone());
        Self {
            kimi,
            semantics,
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Create a new bridge from existing components.
    pub fn from_components(kimi: KimiPrimitives, semantics: DeepSeekSemantics) -> Self {
        Self {
            kimi,
            semantics,
            lock: Arc::new(Mutex::new(())),
        }
    }

    // ── Waiting primitives ──

    /// Poll get_fast_state until the page has input and is not streaming.
    pub async fn wait_until_ready(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut interval = 0.2;

        while Instant::now() < deadline {
            let st = self.semantics.get_fast_state().await;
            if st.has_input && !st.is_streaming {
                debug!(url = %st.url.chars().rev().take(30).collect::<String>().chars().rev().collect::<String>(), "page ready");
                return true;
            }
            tokio::time::sleep(Duration::from_secs_f64(interval)).await;
            interval = (interval * POLL_BACKOFF).min(1.0);
        }

        let st = self.semantics.get_fast_state().await;
        warn!(has_input = st.has_input, is_streaming = st.is_streaming, "wait_until_ready timeout");
        false
    }

    /// Wait until streaming stops for a sustained period.
    async fn wait_for_response(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut interval = POLL_INITIAL;

        while Instant::now() < deadline {
            self.semantics.scroll_virtual_list().await;
            let st = self.semantics.get_fast_state().await;
            if !st.is_streaming {
                tokio::time::sleep(Duration::from_millis(800)).await;
                if !self.semantics.get_fast_state().await.is_streaming {
                    debug!("streaming finished");
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_secs_f64(interval)).await;
            interval = (interval * POLL_BACKOFF).min(POLL_CAP);
        }

        warn!("wait_for_response timeout");
        false
    }

    // ── High-level commands ──

    /// Full send pipeline: ensure_tab → wait → send → wait_for_buttons → extract.
    /// Every send is atomic — just manage the conversation, not the round number.
    pub async fn cmd_send(&self, msg: &str) -> Result<String> {
        let _guard = self.lock.lock().await;

        info!(msg = %msg.chars().take(80).collect::<String>(), "send");

        // Ensure we're on a valid page
        self.semantics.ensure_tab().await?;

        // Wait until page is ready to accept input
        if !self.wait_until_ready(Duration::from_secs_f64(TIMEOUT_READY)).await {
            return Err(AdapterError::PageNotReady {
                reason: "page not ready after timeout".into(),
            });
        }

        // Capture baseline content (to reject stale/duplicate extraction)
        let baseline_content = self.semantics.extract_last_response().await;

        // Send the message
        self.semantics.send_message(msg).await?;

        // Wait for action buttons — the definitive "response fully rendered" signal
        if !self.wait_for_response(Duration::from_secs_f64(TIMEOUT_STREAM_FINISH)).await {
            return Err(AdapterError::NoResponse);
        }

        // Fast-fail: if page already shows a service error, don't bother extracting
        if let Some(err_msg) = self.semantics.check_service_error().await {
            warn!(error = %err_msg, "service error after streaming stopped");
            return Err(AdapterError::NoResponse);
        }

        // Extract with backoff + duplicate guard + stability check
        let mut retry_interval = 0.2;
        for i in 0..EXTRACT_RETRIES {
            // Fast-fail: check for rate-limit or server-busy before wasting retries
            if let Some(err_msg) = self.semantics.check_service_error().await {
                warn!(error = %err_msg, "service error detected, aborting extract");
                return Err(AdapterError::NoResponse);
            }

            tokio::time::sleep(Duration::from_secs_f64(retry_interval)).await;
            self.semantics.scroll_virtual_list().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let content = self.semantics.extract_last_response().await;
            if !content.is_empty() && content != baseline_content {
                // Stability: wait cooldown, re-extract, must get same content
                tokio::time::sleep(Duration::from_secs_f64(COOLDOWN_SECS)).await;
                self.semantics.scroll_virtual_list().await;
                tokio::time::sleep(Duration::from_millis(200)).await;
                let confirm = self.semantics.extract_last_response().await;
                if confirm == content {
                    info!(len = content.len(), retry = i, "send ok");
                    return Ok(content);
                }
                debug!(retry = i, "content unstable, retrying");
            }
            retry_interval = (retry_interval * 1.5).min(2.0);
        }

        warn!("no response after {} fallbacks", EXTRACT_RETRIES);
        let blog = self.semantics.get_browser_log(false).await;
        crate::logging::log_browser_entries(&blog, 5);

        Err(AdapterError::NoResponse)
    }

    /// Get page state summary.
    pub async fn cmd_state(&self) -> Result<String> {
        let _guard = self.lock.lock().await;
        self.semantics.ensure_tab().await?;
        let st = self.semantics.get_fast_state().await;
        let summary = format!(
            "input={} stream={} msgs={} has_chat={} conv_url={}",
            st.has_input,
            st.is_streaming,
            st.message_count,
            st.has_conversation,
            st.url.contains("/a/chat/s/")
        );
        debug!(state = %summary, "state");
        Ok(summary)
    }

    /// Start a new conversation.
    pub async fn cmd_new(&self) -> Result<String> {
        let _guard = self.lock.lock().await;
        info!("new conversation");
        self.semantics.new_conversation().await?;
        if self.wait_until_ready(Duration::from_secs_f64(TIMEOUT_READY)).await {
            Ok("ok".into())
        } else {
            Err(AdapterError::PageNotReady {
                reason: "new conversation page not ready".into(),
            })
        }
    }

    /// Toggle a feature (thinking or search).
    pub async fn cmd_toggle(&self, feature: Feature) -> Result<String> {
        let _guard = self.lock.lock().await;
        info!(feature = ?feature, "toggle");
        self.semantics.ensure_tab().await?;
        self.semantics.toggle_feature(feature).await;
        Ok("ok".into())
    }

    /// Switch chat mode.
    pub async fn cmd_mode(&self, mode: ChatMode) -> Result<String> {
        let _guard = self.lock.lock().await;
        info!(mode = ?mode, "mode");
        self.semantics.ensure_tab().await?;
        self.semantics.select_mode(mode).await;
        Ok("ok".into())
    }

    /// Evaluate arbitrary JS in the browser (for debugging).
    pub async fn cmd_eval(&self, code: &str) -> String {
        let (result, _) = self.kimi.eval_js(code).await;
        result
    }
}

impl std::fmt::Debug for DeepSeekAgentBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepSeekAgentBridge")
            .field("session", &self.kimi.session())
            .finish()
    }
}
