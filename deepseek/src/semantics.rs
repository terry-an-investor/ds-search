//! Layer 2: DeepSeekSemantics — DeepSeek-specific DOM operations via JS eval.
//!
//! Maps to the Python `DeepSeekSemantics` class. All page interactions
//! (send, extract, toggle, mode switch, logging) happen here.

use crate::models::{BrowserLogEntry, ChatMode, FastState, Feature};
use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;
use tracing::debug;

/// DeepSeek-specific page operations.
#[derive(Debug, Clone)]
pub struct DeepSeekSemantics {
    pub kimi: KimiPrimitives,
}

impl DeepSeekSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ── Browser-side logging ──

    /// Install browser-side console/network logging (monkey-patches console.*
    /// and window.fetch).
    pub async fn install_browser_logging(&self) {
        let _ = self.kimi.eval_js(r#"
        (() => {
            if (window.__dsLogPatched) return;
            window.__dsLog = [];
            window.__dsLogMax = 500;
            function cap() {
                if (window.__dsLog.length > window.__dsLogMax)
                    window.__dsLog.splice(0, 100);
            }
            const origLog = console.log;
            const origWarn = console.warn;
            const origError = console.error;
            console.log = function(...a) {
                window.__dsLog.push({lvl:'log', t:Date.now(), m:a.map(x=>typeof x==='string'?x:JSON.stringify(x)).join(' ').substring(0,500)});
                cap();
                return origLog.apply(console, a);
            };
            console.warn = function(...a) {
                window.__dsLog.push({lvl:'warn', t:Date.now(), m:a.map(x=>typeof x==='string'?x:JSON.stringify(x)).join(' ').substring(0,500)});
                cap();
                return origWarn.apply(console, a);
            };
            console.error = function(...a) {
                window.__dsLog.push({lvl:'error', t:Date.now(), m:a.map(x=>typeof x==='string'?x:JSON.stringify(x)).join(' ').substring(0,500)});
                cap();
                return origError.apply(console, a);
            };
            const origFetch = window.fetch;
            window.fetch = function(input, init) {
                const url = typeof input === 'string' ? input : (input.url || '');
                window.__dsLog.push({lvl:'fetch', t:Date.now(), m:url.substring(0,200)});
                cap();
                return origFetch.apply(this, arguments).then(r => {
                    window.__dsLog.push({lvl:'fetch-done', t:Date.now(), m:url.substring(0,100) + ' status=' + r.status});
                    cap();
                    return r;
                }).catch(e => {
                    window.__dsLog.push({lvl:'fetch-err', t:Date.now(), m:url.substring(0,100) + ' ' + e.message});
                    cap();
                    throw e;
                });
            };
            window.__dsLogPatched = true;
        })()
        "#).await;
        debug!("browser logging installed");
    }

    /// Retrieve accumulated browser-side log entries and optionally clear them.
    pub async fn get_browser_log(&self, clear: bool) -> Vec<BrowserLogEntry> {
        let data = self.kimi.eval_json(
            r#"JSON.stringify({
                entries: (window.__dsLog || []).slice(-50),
                count: (window.__dsLog || []).length
            })"#
        ).await;

        if clear {
            let _ = self.kimi.eval_js("window.__dsLog = [];").await;
        }

        match data {
            Some(v) => {
                let entries = v
                    .get("entries")
                    .and_then(|e| e.as_array())
                    .cloned()
                    .unwrap_or_default();
                entries
                    .into_iter()
                    .filter_map(|e| serde_json::from_value::<BrowserLogEntry>(e).ok())
                    .collect()
            }
            None => vec![],
        }
    }

    // ── Fast state check ──

    /// Single JS eval returning lightweight page state.
    /// Suitable for tight polling loops.
    pub async fn get_fast_state(&self) -> FastState {
        let data = self.kimi.eval_json(r#"
        (function() {
            const textarea = document.querySelector('textarea');
            const spinner = document.querySelector(
                '.ds-loading, [class*="loading"], [class*="spinner"], [aria-busy="true"]'
            );
            // Scroll virtual list containers AND message containers
            document.querySelectorAll('.ds-virtual-list, .ds-scroll-area').forEach(el => {
                try { if (el.scrollHeight > el.clientHeight) el.scrollTop = el.scrollHeight; } catch(e) {}
            });
            document.querySelectorAll('*').forEach(el => {
                try {
                    const cs = window.getComputedStyle(el);
                    if ((cs.overflowY !== 'scroll' && cs.overflowY !== 'auto') ||
                        el.scrollHeight <= el.clientHeight + 5) return;
                    if (!el.querySelector('.ds-message') && !(el.className && el.className.includes('ds-virtual-list'))) return;
                    el.scrollTop = el.scrollHeight;
                } catch(e) {}
            });
            const newCount = document.querySelectorAll('.ds-markdown.ds-assistant-message-main-content').length;
            const legacyCount = document.querySelectorAll('.ds-message').length;
            return JSON.stringify({
                has_input: !!textarea,
                is_streaming: !!spinner,
                message_count: newCount || legacyCount,
                url: window.location.href,
                has_conversation: window.location.href.includes('/a/chat/s/'),
                title: document.title
            });
        })()
        "#).await;

        match data {
            Some(v) => FastState {
                has_input: v.get("has_input").and_then(|b| b.as_bool()).unwrap_or(false),
                is_streaming: v.get("is_streaming").and_then(|b| b.as_bool()).unwrap_or(false),
                message_count: v
                    .get("message_count")
                    .and_then(|n| n.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(0),
                url: v.get("url").and_then(|s| s.as_str()).unwrap_or("").into(),
                has_conversation: v
                    .get("has_conversation")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                title: v.get("title").and_then(|s| s.as_str()).unwrap_or("").into(),
            },
            None => FastState::default(),
        }
    }

    // ── Tab management ──

    /// Ensure we're on a valid chat.deepseek.com page.
    /// Detects and recovers from broken SPA state (empty virtual list).
    pub async fn ensure_tab(&self) -> Result<()> {
        // Loop at most twice: first attempt, then one recovery if SPA was stale
        for attempt in 0..2 {
            let url = self.kimi.get_url().await;
            if !url.contains("chat.deepseek.com") {
                break; // need to open/reopen tab (handled below)
            }
            // Verify the page is fully interactive (textarea rendered)
            let (val, _) = self.kimi.eval_js("!!document.querySelector('textarea')").await;
            if val != "true" {
                // textarea missing — page still loading, wait
                debug!("on deepseek domain but no textarea yet, waiting for hydration");
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let (v, _) = self.kimi.eval_js("!!document.querySelector('textarea')").await;
                    if v == "true" {
                        break;
                    }
                }
            }
            // Check for broken virtual list state
            if url.contains("/a/chat/s/") {
                let (healthy, _) = self.kimi.eval_js(
                    "(() => { const vl = document.querySelector('.ds-virtual-list'); if (!vl) return 'no_vl'; return vl.scrollHeight > vl.clientHeight + 5 ? 'ok' : (document.querySelector('.ds-virtual-list-visible-items')?.children.length > 0 ? 'ok' : 'stale'); })()"
                ).await;
                if healthy == "stale" && attempt == 0 {
                    debug!("broken SPA state detected, closing and reopening tab");
                    let _ = self.kimi.close_tab(0).await;
                    continue; // retry with fresh tab
                }
            }
            self.install_browser_logging().await;
            return Ok(());
        }
        if self.kimi.find_tab("https://chat.deepseek.com").await {
            self.kimi.navigate("https://chat.deepseek.com", false).await?;
        } else {
            self.kimi.navigate("https://chat.deepseek.com", true).await?;
        }
        self.install_browser_logging().await;
        Ok(())
    }

    /// Close all inactive DeepSeek tabs from previous sessions.
    pub async fn close_stale_tabs(&self) {
        let tabs = self.kimi.list_tabs().await;
        let current_url = self.kimi.get_url().await;
        for t in tabs {
            if t.url == current_url {
                continue;
            }
            if t.url.contains("chat.deepseek.com") {
                let _ = self.kimi.close_tab(t.tab_id).await;
            }
        }
    }

    // ── Conversation management ──

    /// Start a new conversation: navigate to root URL.
    /// Waits for the page to fully settle and the virtual list to render.
    pub async fn new_conversation(&self) -> Result<()> {
        self.close_stale_tabs().await;
        self.kimi.navigate("https://chat.deepseek.com/", false).await?;

        // Wait for navigation to settle + textarea to appear
        for _ in 0..30 {
            let url = self.kimi.get_url().await;
            let (has_ta, _) = self.kimi.eval_js("!!document.querySelector('textarea')").await;
            if url.trim_end_matches('/') == "https://chat.deepseek.com" && has_ta == "true" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        // Extra time for SPA hydration
        tokio::time::sleep(Duration::from_secs(2)).await;
        self.scroll_virtual_list().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.install_browser_logging().await;
        Ok(())
    }

    // ── Mode / toggle ──

    /// Select chat mode. Returns true on success.
    pub async fn select_mode(&self, mode: ChatMode) -> bool {
        let label = mode.as_label();
        let (ok, _) = self.kimi.eval_js(&format!(
            r#"(() => {{
                const radios = document.querySelectorAll('[role="radio"]');
                for (const r of radios) {{
                    if (r.textContent.includes('{}')) {{
                        r.click();
                        r.setAttribute('aria-checked', 'true');
                        return true;
                    }}
                }}
                return false;
            }})()"#,
            label
        )).await;
        ok == "true"
    }

    /// Toggle a feature (thinking or search). Returns true on success.
    pub async fn toggle_feature(&self, feature: Feature) -> bool {
        let k = feature.as_label();
        let (ok, _) = self.kimi.eval_js(&format!(
            r#"(() => {{
                const el = Array.from(document.querySelectorAll('[role="radio"], button, [role="button"]'))
                    .find(el => (el.textContent||'').includes('{}'));
                if (!el) return false;
                el.click();
                return true;
            }})()"#,
            k
        )).await;
        ok == "true"
    }

    // ── Send message ──

    /// Send a message via native DOM setter + input event + Enter keydown.
    /// Returns true if the send was dispatched successfully.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AdapterError::SendFailed {
                reason: "empty message".into(),
            });
        }
        let safe = serde_json::to_string(text)?;

        // Step 1: fill textarea using native value setter + input event
        let (ok1, _) = self.kimi.eval_js(&format!(
            r#"(() => {{
                const ta = document.querySelector('textarea');
                if (!ta) return 'no-ta';
                ta.focus();
                const pd = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value');
                if (pd && pd.set) {{
                    pd.set.call(ta, {});
                }} else {{
                    ta.value = {};
                }}
                ta.dispatchEvent(new Event('input', {{bubbles: true}}));
                return 'ok';
            }})()"#,
            safe, safe
        )).await;

        if ok1.contains("no-ta") {
            return Err(AdapterError::ElementNotFound {
                selector: "textarea".into(),
            });
        }

        tokio::time::sleep(Duration::from_millis(150)).await;

        // Step 2: dispatch Enter keydown
        let (ok2, _) = self.kimi.eval_js(
            r#"(() => {
                const ta = document.querySelector('textarea');
                if (!ta) return 'no-ta';
                ta.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
                    bubbles: true, cancelable: true
                }));
                return 'ok';
            })()"#
        ).await;

        if ok2.contains("no-ta") {
            return Err(AdapterError::ElementNotFound {
                selector: "textarea".into(),
            });
        }

        debug!(msg_len = text.len(), msg = %text.chars().take(60).collect::<String>(), "message sent");
        Ok(())
    }

    // ── Scroll virtual list ──

    /// Scroll virtual list and message containers to trigger rendering.
    pub async fn scroll_virtual_list(&self) {
        let _ = self.kimi.eval_js(r#"
        (() => {
            // Target virtual list and scroll areas directly (these trigger lazy loading)
            document.querySelectorAll('.ds-virtual-list, .ds-scroll-area').forEach(el => {
                try { if (el.scrollHeight > el.clientHeight) el.scrollTop = el.scrollHeight; } catch(e) {}
            });
            // Also scroll any other container that has messages
            document.querySelectorAll('*').forEach(el => {
                try {
                    const cs = window.getComputedStyle(el);
                    if ((cs.overflowY !== 'scroll' && cs.overflowY !== 'auto') ||
                        el.scrollHeight <= el.clientHeight + 5) return;
                    if (!el.querySelector('.ds-message')) return;
                    el.scrollTop = el.scrollHeight;
                } catch(e) {}
            });
        })()
        "#).await;
    }

    // ── Response extraction ──

    /// Extract the latest assistant response text.
    pub async fn extract_last_response(&self) -> String {
        // Scroll virtual list first to ensure messages render in DOM
        self.scroll_virtual_list().await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let data = self.kimi.eval_json(r#"
        JSON.stringify((() => {
            // Strategy 1: .ds-markdown.ds-assistant-message-main-content (latest first)
            const allResp = document.querySelectorAll('.ds-markdown.ds-assistant-message-main-content');
            for (let i = allResp.length - 1; i >= 0; i--) {
                const t = allResp[i].textContent.trim();
                if (t) return {exists: true, content: t.substring(0, 20000)};
            }

            // Strategy 2: Legacy ds-message fallback
            const msgs = document.querySelectorAll('.ds-message');
            for (let i = msgs.length - 1; i >= 0; i--) {
                const classes = (msgs[i].className || '').split(/\s+/).filter(Boolean);
                const hasHash = classes.some(c => /^d[0-9a-f]{7,}$/i.test(c));
                if (!hasHash) {
                    let content = '';
                    msgs[i].querySelectorAll('.ds-markdown').forEach(md => {
                        if (!md.closest('.ds-think-content')) content = md.textContent.trim();
                    });
                    content = content.replace(/^(正在思考\s*)+/, '')
                        .replace(/^已思考[（(]用时\s*\d+\s*秒[）)]\s*/, '');
                    if (content) return {exists: true, content: content.substring(0, 20000)};
                }
            }
            return {exists: false, content: ''};
        })())
        "#).await;

        match data {
            Some(v) if v.get("exists").and_then(|b| b.as_bool()).unwrap_or(false) => {
                v.get("content")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string()
            }
            _ => String::new(),
        }
    }

    /// Check if the send button is enabled.
    /// During streaming, the send button gets `ds-icon-button--disabled` and
    /// becomes a grey rectangle. When the response is fully rendered, the class
    /// is removed and the button returns to the normal arrow icon.
    /// This is the definitive "can send now" signal.
    pub async fn send_button_enabled(&self) -> bool {
        let (val, _) = self.kimi.eval_js(
            r#"(() => {
                const ta = document.querySelector('textarea');
                if (!ta) return true; // no textarea, assume ready
                // Find input area containing both textarea and send button
                let area = ta.closest('[class*="ec4f"]') || ta.closest('[class*="bf38"]');
                if (!area) {
                    // Fallback: walk up a few levels
                    area = ta.parentElement?.parentElement?.parentElement;
                }
                if (!area) return true;
                // If any ds-icon-button--disabled exists in the input area, send button is disabled
                return area.querySelector('.ds-icon-button--disabled') === null;
            })()"#
        ).await;
        val == "true"
    }
    /// Check if the page is showing a service-level error (rate limit, server busy, etc).
    /// Returns Some(error_message) if an error is detected, None if the page looks healthy.
    pub async fn check_service_error(&self) -> Option<String> {
        let (val, _) = self.kimi.eval_js(r#"
        (() => {
            const t = document.body.innerText || '';
            if (t.includes('消息发送过于频繁')) return 'rate limited: 消息发送过于频繁，请稍后重试';
            if (t.includes('服务器繁忙')) return 'server busy: 服务器繁忙，请稍后重试';
            return '';
        })()
        "#).await;
        if val.is_empty() { None } else { Some(val) }
    }

    pub async fn extract_thinking(&self) -> Option<String> {
        let raw = self.kimi.eval_js(r#"
        (() => {
            const msgs = document.querySelectorAll('.ds-message');
            for (let i = msgs.length - 1; i >= 0; i--) {
                const cls = (msgs[i].className || '').split(/\s+/);
                if (cls.includes('ds-message') && !cls.some(c => /^d[0-9a-f]{7,}$/i.test(c))) {
                    const th = msgs[i].querySelector('.ds-think-content');
                    if (th) {
                        const parts = Array.from(th.querySelectorAll('.ds-markdown'))
                            .map(md => md.textContent.trim());
                        const full = msgs[i].textContent;
                        const tm = full.match(/已思考[（(]用时\s*(\d+)\s*秒[）)]/);
                        return (tm ? '[思考 ' + tm[1] + 's] ' : '') + parts.join('\n').substring(0, 10000);
                    }
                    return '';
                }
            }
            return '';
        })()
        "#).await;

        let text = raw.0.trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

}

impl From<KimiPrimitives> for DeepSeekSemantics {
    fn from(kimi: KimiPrimitives) -> Self {
        Self::new(kimi)
    }
}
