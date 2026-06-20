//! Layer 2: GrokSemantics — Grok-specific DOM operations via JS eval.
//!
//! All page interactions (send, extract, toggle) happen here.
//! Uses KimiPrimitives from ds-adapter for browser control.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;
use tracing::debug;

use crate::models::GrokState;

const GROK_URL: &str = "https://x.com/i/grok";

/// Grok-specific page operations.
#[derive(Debug, Clone)]
pub struct GrokSemantics {
    pub kimi: KimiPrimitives,
}

impl GrokSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ── State check ──

    /// Quick page state observation.
    pub async fn get_state(&self) -> GrokState {
        let data = self
            .kimi
            .eval_json(
                r#"JSON.stringify((()=>{
                const ta=document.querySelector('textarea');
                return {
                    has_input: !!ta,
                    has_conversation: window.location.href.includes('conversation='),
                    url: window.location.href,
                    is_initialized: !document.body.innerText.includes('Talk to Grok')
                };
            })())"#,
            )
            .await;

        match data {
            Some(v) => GrokState {
                has_input: v
                    .get("has_input")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                has_conversation: v
                    .get("has_conversation")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                url: v.get("url").and_then(|s| s.as_str()).unwrap_or("").into(),
                is_initialized: v
                    .get("is_initialized")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
            },
            None => GrokState::default(),
        }
    }

    // ── Tab management ──

    /// Ensure we're on a valid Grok page, initialize chat if needed.
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;

        if !url.contains("x.com/i/grok") {
            if self.kimi.find_tab(GROK_URL).await {
                self.kimi.navigate(GROK_URL, false).await?;
            } else {
                self.kimi.navigate(GROK_URL, true).await?;
            }
        }

        // Wait for page to hydrate (textarea must exist)
        for _ in 0..20 {
            let (has_ta, _) = self
                .kimi
                .eval_js("!!document.querySelector('textarea')")
                .await;
            if has_ta == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Dismiss "Talk to Grok" banner if present
        let (has_banner, _) = self
            .kimi
            .eval_js("document.body.innerText.includes('Talk to Grok') ? 'true' : 'false'")
            .await;
        if has_banner == "true" {
            debug!("dismissing Talk to Grok banner");
            let _ = self
                .kimi
                .eval_js(
                    r#"(()=>{
                    const btns=Array.from(document.querySelectorAll('button,[role=button]'));
                    const talk=btns.find(b=>(b.textContent||'').includes('Talk to Grok'));
                    if(talk){talk.click();return'clicked'}
                    return'not found';
                })()"#,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    // ── Send message ──

    /// Send a message: clear textarea, key_type text, send_keys Enter.
    /// Returns Ok if the send was dispatched.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AdapterError::SendFailed {
                reason: "empty message".into(),
            });
        }

        // Step 1: Clear any stale text from previous failed sends
        let _ = self
            .kimi
            .eval_js(
                r#"(()=>{
                const ta=document.querySelector('textarea');
                if(ta){ta.focus();ta.value='';ta.dispatchEvent(new Event('input',{bubbles:true}));}
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Step 2: Type the message via OS-level keyboard
        self.kimi.key_type(text).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Step 3: Verify text is in the textarea
        let (val, _) = self
            .kimi
            .eval_js("document.querySelector('textarea')?.value || ''")
            .await;
        if !val.contains(text) && val.len() < text.len() {
            debug!(
                expected = text,
                actual = val,
                "key_type may not have landed"
            );
            return Err(AdapterError::SendFailed {
                reason: format!("text not in textarea after key_type: '{}'", val),
            });
        }

        // Step 4: Send via Enter
        self.kimi.send_keys("Enter").await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Step 5: Verify textarea was cleared (indicating send went through)
        let (after_val, _) = self
            .kimi
            .eval_js("document.querySelector('textarea')?.value || ''")
            .await;
        if !after_val.is_empty() {
            debug!("textarea not cleared after Enter, trying fallback click on send button");
            // Fallback: click the icon-only send button
            let _ = self
                .kimi
                .eval_js(
                    r#"(()=>{
                    const btns=Array.from(document.querySelectorAll('button'));
                    const send=btns.find(b=>{
                        const t=(b.textContent||'').trim();
                        return !t && b.querySelector('svg');
                    });
                    if(send){send.click();return'clicked'}
                    return'no send button';
                })()"#,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        debug!(msg_len = text.len(), "message sent");
        Ok(())
    }

    /// Wait for streaming to finish. Polls body text length until stable.
    pub async fn wait_for_response(&self, timeout_secs: u64) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut last_len = 0;
        let mut stable_count = 0;

        loop {
            let (len_str, _) = self.kimi.eval_js("document.body.innerText.length").await;
            let len: usize = len_str.parse().unwrap_or(0);

            if len == last_len {
                stable_count += 1;
                if stable_count >= 4 {
                    return true;
                }
            } else {
                last_len = len;
                stable_count = 0;
            }

            if tokio::time::Instant::now() > deadline {
                debug!("wait_for_response timeout");
                return false;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Extract the latest Grok response from the page body.
    /// Grok renders responses as plain text — no markdown containers like DeepSeek.
    pub async fn extract_last_response(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            r#"JSON.stringify((()=>{
                const body=document.body.innerText;
                // Find conversation ID in URL to locate our conversation block
                const url=window.location.href;
                const match=url.match(/conversation=(\d+)/);
                if(!match)return {found:false,text:''};
                // Grok responses appear after the last user message.
                // Strategy: find the last user message, then grab text until next interactive element.
                const lines=body.split('\n');
                // Reverse scan: find lines that look like user messages (shorter, no bullet points)
                // and assistant responses (longer, may have formatting)
                // Simple approach: find the last conversational block
                let lastBlock='';
                let inResponse=false;
                for(let i=lines.length-1;i>=0;i--){
                    const l=lines[i].trim();
                    if(!l)continue;
                    // Stop at UI elements
                    if(['Fast','History','Create Images','Edit Image','Latest News','See new posts','Home','Explore'].includes(l))continue;
                    if(l.startsWith('Explore ')||l.startsWith('What ')||l.startsWith('Learn ')||l.startsWith('Classic'))continue;
                    if(l.length>10&&!l.includes('@')&&!l.includes('View keyboard')){
                        lastBlock=l+'\n'+lastBlock;
                    }
                    if(lastBlock.length>100)break;
                }
                return {found:true,text:lastBlock.trim().substring(0,5000)};
            })())"#,
        ).await;

        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) if v.get("found").and_then(|b| b.as_bool()).unwrap_or(false) => v
                .get("text")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            _ => String::new(),
        }
    }

    /// Start a new conversation: navigate to base Grok URL.
    pub async fn new_conversation(&self) -> Result<()> {
        self.kimi.navigate(GROK_URL, false).await?;
        for _ in 0..20 {
            let (has_ta, _) = self
                .kimi
                .eval_js("!!document.querySelector('textarea')")
                .await;
            if has_ta == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }
}
