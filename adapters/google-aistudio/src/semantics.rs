//! AistudioSemantics — Google AI Studio operations.
//!
//! Features: send prompts, extract responses, select models, browse history.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;

use crate::models::AistudioModel;

const PLAYGROUND_URL: &str = "https://aistudio.google.com/prompts/new_chat";
const HISTORY_URL: &str = "https://aistudio.google.com/library";

#[derive(Debug, Clone)]
pub struct AistudioSemantics {
    pub kimi: KimiPrimitives,
}

impl AistudioSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ═══════════════════════════════════════════════════════════
    // Tab management
    // ═══════════════════════════════════════════════════════════

    /// Ensure we have an AI Studio tab open (any page).
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("aistudio.google.com") {
            self.kimi.navigate(PLAYGROUND_URL, false).await?;
            tokio::time::sleep(Duration::from_millis(3000)).await;
        }
        Ok(())
    }

    /// Navigate to the playground (new chat).
    pub async fn go_playground(&self) -> Result<()> {
        self.kimi.navigate(PLAYGROUND_URL, false).await?;
        tokio::time::sleep(Duration::from_millis(3000)).await;
        Ok(())
    }

    /// Navigate to the history / library page.
    pub async fn go_history(&self) -> Result<()> {
        self.kimi.navigate(HISTORY_URL, false).await?;
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Prompting
    // ═══════════════════════════════════════════════════════════

    /// Check if we're on a playground page.
    pub async fn is_on_playground(&self) -> bool {
        let url = self.kimi.get_url().await;
        url.contains("/prompts/")
    }

    /// Set the prompt textarea value and dispatch input events.
    async fn set_prompt_text(&self, text: &str) -> Result<()> {
        let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
        let code = format!(
            r#"(() => {{
                const ta = document.querySelector('textarea');
                if (!ta) return 'no_textarea';
                const nativeSetter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set;
                nativeSetter.call(ta, '{}');
                ta.dispatchEvent(new Event('input', {{bubbles: true}}));
                ta.dispatchEvent(new Event('change', {{bubbles: true}}));
                return 'ok';
            }})()"#,
            escaped
        );
        let (result, _) = self.kimi.eval_js(&code).await;
        if result.contains("no_textarea") {
            return Err(AdapterError::ElementNotFound { selector: "textarea".into() });
        }
        Ok(())
    }

    /// Click the Run button.
    async fn click_run(&self) -> Result<()> {
        let (found, _) = self.kimi.eval_js(
            r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.textContent.includes('Run') && !b.disabled);
                if (btn) { btn.click(); return 'ok'; }
                return 'not_found';
            })()"#,
        ).await;
        if found.contains("not_found") {
            return Err(AdapterError::ElementNotFound { selector: "Run button".into() });
        }
        Ok(())
    }

    /// Send a prompt by setting the textarea and clicking Run.
    pub async fn send_prompt(&self, text: &str) -> Result<()> {
        self.ensure_tab().await?;
        self.set_prompt_text(text).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.click_run().await?;
        Ok(())
    }

    /// Wait for a response to complete (poll until no spinner, stable text).
    pub async fn wait_for_response(&self, timeout_secs: u64) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut last_len = 0usize;
        let mut stable = 0u32;

        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let (text, _) = self.kimi.eval_js(
                "document.querySelector('.chat-turn-container.model:last-of-type')?.textContent?.length || 0"
            ).await;
            let len: usize = text.trim().parse().unwrap_or(0);

            if len > 0 && len == last_len {
                stable += 1;
                if stable >= 3 {
                    return Ok(true); // response ready
                }
            } else {
                last_len = len;
                stable = 0;
            }

            if tokio::time::Instant::now() > deadline {
                return Ok(len > 0);
            }
        }
    }

    /// Extract the response text from the page body.
    /// The response appears in document.body.innerText between "Model" timestamp and "thumb_up".
    pub async fn extract_response(&self) -> String {
        let (text, _) = self.kimi.eval_js(
            r#"(() => {
                const bodyText = document.body.innerText;
                // Find the last "Model" timestamp marker
                const modelMarkers = [...bodyText.matchAll(/\nModel\s+\d/g)];
                if (modelMarkers.length === 0) return '';
                const lastMarker = modelMarkers[modelMarkers.length - 1];
                const startIdx = lastMarker.index + lastMarker[0].length;
                // Find the next newline after the timestamp
                const afterMarker = bodyText.substring(startIdx);
                const nlIdx = afterMarker.indexOf('\n');
                const contentStart = startIdx + (nlIdx >= 0 ? nlIdx + 1 : 0);
                const after = bodyText.substring(contentStart);
                // Stop at thumb_up/thumb_down or disclaimer
                const endMarkers = ['\nthumb_up', '\nthumb_down', '\ninfo\nGoogle AI models'];
                let endIdx = after.length;
                for (const m of endMarkers) {
                    const idx = after.indexOf(m);
                    if (idx >= 0 && idx < endIdx) endIdx = idx;
                }
                return after.substring(0, endIdx).trim();
            })()"#,
        ).await;
        text
    }

    // ═══════════════════════════════════════════════════════════
    // Model selection
    // ═══════════════════════════════════════════════════════════

    /// Get the currently selected model name from the settings panel.
    pub async fn current_model(&self) -> String {
        let (text, _) = self.kimi.eval_js(
            r#"(() => {
                const card = document.querySelector('.model-selector-card');
                if (!card) return '(unknown)';
                const title = card.querySelector('.model-title-text, [class*=model-title]');
                if (title) return title.textContent.trim();
                // Fallback: get just the first line (model name)
                const lines = card.textContent.trim().split('\n');
                return lines.length > 0 ? lines[0].trim() : '(unknown)';
            })()"#,
        ).await;
        // Trim trailing model ID (starts with "gemini-")
        if let Some(idx) = text.find("gemini-") {
            text[..idx].trim().to_string()
        } else {
            text
        }
    }

    /// Open the model selection dialog.
    pub async fn open_model_selector(&self) -> Result<()> {
        let (result, _) = self.kimi.eval_js(
            r#"(() => {
                const card = document.querySelector('.model-selector-card');
                if (!card) return 'no_selector';
                card.click();
                return 'ok';
            })()"#,
        ).await;
        if result.contains("no_selector") {
            return Err(AdapterError::ElementNotFound { selector: ".model-selector-card".into() });
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(())
    }

    /// Select a model by clicking its card in the open dialog.
    pub async fn select_model(&self, model: AistudioModel) -> Result<()> {
        self.open_model_selector().await?;

        let model_id = model.model_id();
        let escaped = model_id.replace('\\', "\\\\").replace('\'', "\\'");
        let code = format!(
            r#"(() => {{
                const dialog = document.querySelector('.mat-mdc-dialog-content');
                if (!dialog) return 'no_dialog';
                const buttons = dialog.querySelectorAll('button, [role=option]');
                for (const btn of buttons) {{
                    if (btn.textContent.includes('{}')) {{
                        btn.click();
                        return 'selected';
                    }}
                }}
                return 'not_found';
            }})()"#,
            escaped
        );
        let (result, _) = self.kimi.eval_js(&code).await;
        match result.as_str() {
            "selected" => Ok(()),
            "no_dialog" => Err(AdapterError::PageNotReady { reason: "model dialog not open".into() }),
            _ => Err(AdapterError::SendFailed { reason: format!("model '{}' not found in dialog", model_id) }),
        }
    }

    /// Set the thinking level (Low/Medium/High).
    pub async fn set_thinking_level(&self, level: &str) -> Result<()> {
        let escaped = level.replace('\\', "\\\\").replace('\'', "\\'");
        let code = format!(
            r#"(() => {{
                const select = document.querySelector('mat-select, [role=combobox]');
                if (!select) return 'no_select';
                select.click();
                setTimeout(() => {{
                    const options = document.querySelectorAll('mat-option, [role=option]');
                    for (const opt of options) {{
                        if (opt.textContent.trim() === '{}') {{
                            opt.click();
                        }}
                    }}
                }}, 300);
                return 'opened';
            }})()"#,
            escaped
        );
        let (result, _) = self.kimi.eval_js(&code).await;
        if result.contains("no_select") {
            return Err(AdapterError::ElementNotFound { selector: "thinking level selector".into() });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Chat management
    // ═══════════════════════════════════════════════════════════

    /// Start a new chat by clicking the New chat button.
    pub async fn new_chat(&self) -> Result<()> {
        self.ensure_tab().await?;
        let (result, _) = self.kimi.eval_js(
            r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.getAttribute('aria-label') === 'New chat');
                if (btn) { btn.click(); return 'ok'; }
                return 'not_found';
            })()"#,
        ).await;
        if result.contains("not_found") {
            return Err(AdapterError::ElementNotFound { selector: "New chat button".into() });
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Ok(())
    }

    /// Get the current prompt title (H1).
    pub async fn current_title(&self) -> String {
        let (text, _) = self.kimi.eval_js(
            r#"(() => {
                const h1 = document.querySelector('h1');
                return h1 ? h1.textContent.trim() : '';
            })()"#,
        ).await;
        text
    }

    /// Click the "Get code" button and extract the code snippet.
    pub async fn get_code(&self) -> Result<String> {
        let (result, _) = self.kimi.eval_js(
            r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.textContent.includes('Get code'));
                if (!btn) return 'no_get_code_btn';
                btn.click();
                return 'clicked';
            })()"#,
        ).await;
        if result.contains("no_get_code_btn") {
            return Err(AdapterError::ElementNotFound { selector: "Get code button".into() });
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;

        let (code, _) = self.kimi.eval_js(
            r#"(() => {
                const pre = document.querySelector('pre, code, [class*=code-block], [class*=snippet]');
                return pre ? pre.textContent.trim() : '';
            })()"#,
        ).await;
        Ok(code)
    }

    // ═══════════════════════════════════════════════════════════
    // History
    // ═══════════════════════════════════════════════════════════

    /// Extract prompt history items from the /library page.
    pub async fn extract_history(&self, n: usize) -> Vec<HistoryItem> {
        let (raw, _) = self.kimi.eval_js(&format!(
            r#"JSON.stringify((() => {{
                const links = Array.from(document.querySelectorAll('a'))
                    .filter(a => a.href.includes('/prompts/') && !a.href.endsWith('/new_chat'));
                const items = [];
                for (const a of links) {{
                    if (items.length >= {0}) break;
                    const name = a.textContent.trim();
                    if (!name || name.length < 2) continue;
                    const row = a.closest('[class*=row], [class*=item], tr, div');
                    let time = '';
                    if (row) {{
                        const text = row.textContent;
                        const match = text.match(/(\d+\s+(?:second|minute|hour|day|week|month|year)s?\s+ago|a\s+few\s+seconds\s+ago|just\s+now|A\s+minute\s+ago)/i);
                        time = match ? match[0] : '';
                    }}
                    items.push({{
                        name: name,
                        time: time,
                        url: a.href,
                        snippet: ''
                    }});
                }}
                return items;
            }})())"#,
            n
        )).await;

        let items: Vec<serde_json::Value> =
            serde_json::from_str(&raw).unwrap_or_default();
        items
            .into_iter()
            .map(|v| HistoryItem {
                name: v["name"].as_str().unwrap_or("").to_string(),
                time: v["time"].as_str().unwrap_or("").to_string(),
                url: v["url"].as_str().unwrap_or("").to_string(),
                snippet: v["snippet"].as_str().unwrap_or("").to_string(),
            })
            .collect()
    }

    /// Open a prompt from history by its name (clicks the matching row).
    pub async fn open_history_prompt(&self, name: &str) -> Result<()> {
        if !self.kimi.get_url().await.contains("/library") {
            self.go_history().await?;
        }
        let escaped = name.replace('\\', "\\\\").replace('\'', "\\'");
        let code = format!(
            r#"(() => {{
                const rows = document.querySelectorAll('[class*=row], [class*=item], tr, a');
                for (const row of rows) {{
                    if (row.textContent.includes('{}')) {{
                        const link = row.tagName === 'A' ? row : row.querySelector('a');
                        if (link) {{ link.click(); return 'opened'; }}
                        row.click();
                        return 'clicked';
                    }}
                }}
                return 'not_found';
            }})()"#,
            escaped
        );
        let (result, _) = self.kimi.eval_js(&code).await;
        if result.contains("not_found") {
            return Err(AdapterError::SendFailed { reason: format!("prompt '{}' not found in history", name) });
        }
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Check if the page has an active response (model turn present).
    pub async fn has_response(&self) -> bool {
        let (text, _) = self.kimi.eval_js(
            "document.querySelectorAll('.chat-turn-container.model').length"
        ).await;
        text.trim().parse::<i32>().unwrap_or(0) > 0
    }
}

// ═══════════════════════════════════════════════════════════
// Data types
// ═══════════════════════════════════════════════════════════

/// A prompt history entry from the /library page.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    pub name: String,
    pub time: String,
    pub url: String,
    pub snippet: String,
}
