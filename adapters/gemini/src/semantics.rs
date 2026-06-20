//! GeminiSemantics — Gemini-specific DOM operations via JS eval.
//!
//! Key differences from DeepSeek/Grok:
//! - Input is contenteditable DIV, not textarea
//! - Model picker: .input-area-switch → .gds-mode-switch-menu (Fast/Thinking/Pro)
//! - Thinking: .model-thoughts > .thoughts-content (must click "Show thinking" to expand)
//! - Response: .model-response-text (actual text), .response-container (wrapper)
//! - Streaming: .processing-state-visible class

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

const GEMINI_URL: &str = "https://gemini.google.com/app";
const INPUT_SELECTOR: &str = r#"[role="textbox"][contenteditable="true"]"#;

/// Gemini model options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeminiModel {
    Fast,
    Thinking,
    Pro,
}

impl GeminiModel {
    pub fn as_label(&self) -> &'static str {
        match self {
            GeminiModel::Fast => "Fast",
            GeminiModel::Thinking => "Thinking",
            GeminiModel::Pro => "Pro",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeminiSemantics {
    pub kimi: KimiPrimitives,
}

impl GeminiSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    /// Ensure we're on a valid Gemini page.
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("gemini.google.com") {
            if self.kimi.find_tab(GEMINI_URL).await {
                self.kimi.navigate(GEMINI_URL, false).await?;
            } else {
                self.kimi.navigate(GEMINI_URL, true).await?;
            }
        }
        for _ in 0..20 {
            let (has, _) = self
                .kimi
                .eval_js(&format!(
                    "!!document.querySelector('{}') ? 'true' : 'false'",
                    INPUT_SELECTOR
                ))
                .await;
            if has == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Switch Gemini model. Clicks .input-area-switch, then clicks the model option.
    pub async fn select_model(&self, model: GeminiModel) -> Result<()> {
        let label = model.as_label();

        // Click the model switcher to open dropdown
        let _ = self.kimi.eval_js(
            r#"(()=>{const el=document.querySelector('.input-area-switch');if(el)el.click()})()"#,
        ).await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Click the target model in the menu
        let (result, _) = self.kimi.eval_js(&format!(
            r#"(()=>{{
                const items=document.querySelectorAll('.gds-mode-switch-menu [role=menuitem],.gds-mode-switch-menu button');
                for(const item of items){{
                    if((item.textContent||'').includes('{}')){{
                        item.click();return'clicked';
                    }}
                }}
                return'not found';
            }})()"#,
            label
        )).await;

        if result.contains("not found") {
            return Err(AdapterError::ElementNotFound {
                selector: format!("model '{}' in menu", label),
            });
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(())
    }

    /// Send a message via key_type + Enter.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AdapterError::SendFailed {
                reason: "empty message".into(),
            });
        }

        // Focus and clear
        let _ = self.kimi.eval_js(&format!(
            r#"(()=>{{const el=document.querySelector('{}');if(el){{el.focus();el.textContent='';}}}})()"#,
            INPUT_SELECTOR
        )).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Type
        self.kimi.key_type(text).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Verify
        let (content, _) = self
            .kimi
            .eval_js(&format!(
                "document.querySelector('{}')?.textContent || ''",
                INPUT_SELECTOR
            ))
            .await;
        if !content.contains(text) {
            // Fallback: try fill via dispatch
            let _ = self.kimi.eval_js(&format!(
                r#"(()=>{{const el=document.querySelector('{}');if(el){{el.textContent={};el.dispatchEvent(new Event('input',{{bubbles:true}}));}}}})()"#,
                INPUT_SELECTOR,
                serde_json::to_string(text)?
            )).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // Send via click on send button (Enter doesn't reliably trigger Gemini's React handler)
        let (clicked, _) = self.kimi.eval_js(
            r#"(()=>{const btn=document.querySelector('[aria-label="Send message"]');if(btn){btn.click();return'true';}return'false';})()"#,
        ).await;
        if clicked != "true" {
            // Fallback: try Enter
            self.kimi.send_keys("Enter").await?;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    /// Wait for response by polling response-container count until stable.
    pub async fn wait_for_response(&self, timeout_secs: u64) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let (start_str, _) = self
            .kimi
            .eval_js("document.querySelectorAll('.response-container').length")
            .await;
        let start_count: usize = start_str.parse().unwrap_or(0);
        let mut last_count = start_count;
        let mut stable = 0;

        loop {
            let (s, _) = self
                .kimi
                .eval_js("document.querySelectorAll('.response-container').length")
                .await;
            let count: usize = s.parse().unwrap_or(0);

            if count > start_count {
                if count == last_count {
                    stable += 1;
                    // Also check that processing-state-visible is gone
                    let (processing, _) = self.kimi.eval_js(
                        "!!document.querySelector('.processing-state-visible') ? 'true' : 'false'"
                    ).await;
                    if stable >= 4 && processing != "true" {
                        return true;
                    }
                } else {
                    last_count = count;
                    stable = 0;
                }
            }

            if tokio::time::Instant::now() > deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Extract the latest Gemini response text (no thinking).
    pub async fn extract_last_response(&self) -> String {
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(()=>{
                const containers=document.querySelectorAll('.model-response-text');
                if(containers.length===0)return'';
                return containers[containers.length-1].textContent.trim().substring(0,10000);
            })()"#,
            )
            .await;
        raw.trim().to_string()
    }

    /// Extract thinking content for the latest response.
    /// Expands the "Show thinking" button first if needed.
    pub async fn extract_thinking(&self) -> Option<String> {
        // Find the last .model-thoughts container and expand it
        let _ = self
            .kimi
            .eval_js(
                r#"(()=>{
                const thoughts=document.querySelectorAll('.model-thoughts');
                if(thoughts.length===0)return;
                const last=thoughts[thoughts.length-1];
                const btn=last.querySelector('.thoughts-header-button');
                if(btn && (last.textContent||'').includes('Show thinking')){
                    btn.click();
                }
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Extract thinking content
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(()=>{
                const containers=document.querySelectorAll('.thoughts-content');
                if(containers.length===0)return'';
                const last=containers[containers.length-1];
                let text=last.textContent.trim();
                // Strip "Show thinking" prefix if present
                text=text.replace(/^Show thinking\s*/,'');
                return text.substring(0,10000);
            })()"#,
            )
            .await;

        let text = raw.trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    }

    /// Extract streaming/processing content in real-time.
    /// Returns the current partial response + thinking if available.
    pub async fn get_streaming_state(&self) -> String {
        let (raw, _) = self.kimi.eval_js(
            r#"JSON.stringify((()=>{
                const processing=!!document.querySelector('.processing-state-visible');
                const lastResp=document.querySelectorAll('.model-response-text');
                const respText=lastResp.length>0?lastResp[lastResp.length-1].textContent.trim().substring(0,2000):'';
                const lastThought=document.querySelectorAll('.thoughts-content');
                const thoughtText=lastThought.length>0?lastThought[lastThought.length-1].textContent.trim().replace(/^Show thinking\s*/,'').substring(0,2000):'';
                return {processing:processing,response:respText,thinking:thoughtText};
            })())"#,
        ).await;
        raw
    }

    /// Start a new conversation.
    pub async fn new_conversation(&self) -> Result<()> {
        self.kimi.navigate(GEMINI_URL, false).await?;
        for _ in 0..20 {
            let (has, _) = self
                .kimi
                .eval_js(&format!(
                    "!!document.querySelector('{}') ? 'true' : 'false'",
                    INPUT_SELECTOR
                ))
                .await;
            if has == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }
}
