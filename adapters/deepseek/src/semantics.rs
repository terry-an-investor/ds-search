//! Layer 2: DeepSeekSemantics — DeepSeek-specific DOM operations via JS eval.
//!
//! Maps to the Python `DeepSeekSemantics` class. All page interactions
//! (send, extract, toggle, mode switch, logging) happen here.

use crate::models::{BrowserLogEntry, ChatMode, ChatTurn, FastState, Feature, ThinkingTrace};
use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, info, warn};

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
        let data = self
            .kimi
            .eval_json(
                r#"JSON.stringify({
                entries: (window.__dsLog || []).slice(-50),
                count: (window.__dsLog || []).length
            })"#,
            )
            .await;

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
                has_input: v
                    .get("has_input")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                is_streaming: v
                    .get("is_streaming")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
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
            let (val, _) = self
                .kimi
                .eval_js("!!document.querySelector('textarea')")
                .await;
            if val != "true" {
                // textarea missing — page still loading, wait
                debug!("on deepseek domain but no textarea yet, waiting for hydration");
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let (v, _) = self
                        .kimi
                        .eval_js("!!document.querySelector('textarea')")
                        .await;
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
            self.kimi
                .navigate("https://chat.deepseek.com", false)
                .await?;
        } else {
            self.kimi
                .navigate("https://chat.deepseek.com", true)
                .await?;
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
        self.kimi
            .navigate("https://chat.deepseek.com/", false)
            .await?;

        // Wait for navigation to settle + textarea to appear
        for _ in 0..30 {
            let url = self.kimi.get_url().await;
            let (has_ta, _) = self
                .kimi
                .eval_js("!!document.querySelector('textarea')")
                .await;
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
        let (ok, _) = self
            .kimi
            .eval_js(&format!(
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
            ))
            .await;
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

    /// Send a message via native DOM setter + input event + Enter keydown,
    /// with verification at each step (per AGENTS.md observe→act→verify rule).
    ///
    /// Returns Ok(()) only when the textarea is confirmed cleared after the
    /// Enter dispatch — that is the definitive "message accepted" signal.
    pub async fn send_message(&self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AdapterError::SendFailed {
                reason: "empty message".into(),
            });
        }
        let safe = serde_json::to_string(text)?;

        // ── Step 1: fill textarea via native value setter ──
        let (ok1, _) = self
            .kimi
            .eval_js(&format!(
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
            ))
            .await;
        if ok1.contains("no-ta") {
            return Err(AdapterError::ElementNotFound {
                selector: "textarea".into(),
            });
        }

        tokio::time::sleep(Duration::from_millis(150)).await;

        // ── VERIFY 1: textarea actually contains the text ──
        // If the React setter didn't take, fall back to OS-level key_type.
        let (ta_val, _) = self
            .kimi
            .eval_js("document.querySelector('textarea')?.value || ''")
            .await;
        if !ta_val.contains(text) {
            debug!(
                expected_len = text.len(),
                actual_len = ta_val.len(),
                "fill did not land, falling back to key_type"
            );
            self.kimi.key_type(text).await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            // Re-verify after fallback; if still empty, the page is broken.
            let (ta_val2, _) = self
                .kimi
                .eval_js("document.querySelector('textarea')?.value || ''")
                .await;
            if ta_val2.is_empty() {
                return Err(AdapterError::SendFailed {
                    reason: "textarea empty after fill + key_type fallback".into(),
                });
            }
        }

        // ── Step 2: dispatch Enter keydown ──
        let (_ok2, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const ta = document.querySelector('textarea');
                if (!ta) return 'no-ta';
                ta.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
                    bubbles: true, cancelable: true
                }));
                return 'ok';
            })()"#,
            )
            .await;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // ── VERIFY 2: textarea cleared after Enter ──
        // A cleared textarea is the definitive "message accepted by the app" signal.
        // If not cleared, fall back to clicking the send button.
        let (after_val, _) = self
            .kimi
            .eval_js("document.querySelector('textarea')?.value || ''")
            .await;
        if !after_val.is_empty() {
            debug!("textarea not cleared after Enter, falling back to send button click");
            let (_clicked, _) = self
                .kimi
                .eval_js(
                    r#"(() => {
                    const btns = Array.from(document.querySelectorAll('button'));
                    // DeepSeek's send button is icon-only (no text) and lives in the input area.
                    const send = btns.find(b => !b.textContent.trim() && b.querySelector('svg') && !b.disabled);
                    if (send) { send.click(); return 'clicked'; }
                    return 'no-send-btn';
                })()"#,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(500)).await;
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
            Some(v) if v.get("exists").and_then(|b| b.as_bool()).unwrap_or(false) => v
                .get("content")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            _ => String::new(),
        }
    }

    /// Check if the send button is enabled.
    /// During streaming, the send button gets `ds-icon-button--disabled` and
    /// becomes a grey rectangle. When the response is fully rendered, the class
    /// is removed and the button returns to the normal arrow icon.
    /// This is the definitive "can send now" signal.
    pub async fn send_button_enabled(&self) -> bool {
        let (val, _) = self
            .kimi
            .eval_js(
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
            })()"#,
            )
            .await;
        val == "true"
    }
    /// Check if the page is showing a service-level error (rate limit, server busy, etc).
    /// Returns Some(error_message) if an error is detected, None if the page looks healthy.
    pub async fn check_service_error(&self) -> Option<String> {
        let (val, _) = self
            .kimi
            .eval_js(
                r#"
        (() => {
            const t = document.body.innerText || '';
            if (t.includes('消息发送过于频繁')) return 'rate limited: 消息发送过于频繁，请稍后重试';
            if (t.includes('服务器繁忙')) return 'server busy: 服务器繁忙，请稍后重试';
            return '';
        })()
        "#,
            )
            .await;
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
        if text.is_empty() { None } else { Some(text) }
    }

    // ── Full send pipeline (stability-confirmed) ──

    /// Wait until the page stops streaming. Polls `is_streaming` until false,
    /// then re-checks after a short cooldown to filter out momentary pauses.
    async fn wait_for_response(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut interval = 0.1;

        while tokio::time::Instant::now() < deadline {
            self.scroll_virtual_list().await;
            let st = self.get_fast_state().await;
            if !st.is_streaming {
                // Cooldown: streaming may pause briefly mid-generation
                tokio::time::sleep(Duration::from_millis(800)).await;
                if !self.get_fast_state().await.is_streaming {
                    debug!("streaming finished");
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_secs_f64(interval)).await;
            interval = (interval * 1.3).min(0.5);
        }

        warn!("wait_for_response timeout");
        false
    }

    // ── Turn extraction (full conversation) ──

    /// Extract the full multi-turn conversation as paired user+assistant turns.
    ///
    /// Scrolls the virtual list and retries until the `.ds-message` count stabilizes
    /// (DeepSeek lazy-unmounts off-screen messages, same as AI Studio). Each `.ds-message`
    /// is classified by the hash-class heuristic: hash-prefixed class = user, otherwise = assistant.
    /// Consecutive user+assistant messages are paired into a ChatTurn; a trailing unpaired
    /// user message (e.g. sent but no reply yet) is included with an empty response.
    pub async fn extract_turns(&self) -> Vec<ChatTurn> {
        // ── Fast path: fetch the full conversation via the history API ──
        // DeepSeek exposes GET /api/v0/chat/history_messages?chat_session_id=<id>
        // with the userToken from localStorage. This returns ALL messages in one
        // shot (no virtual-list truncation). Falls back to DOM sweep if the API
        // is unavailable (not logged in, URL has no session id, API changed).
        if let Some(turns) = self.extract_turns_via_api().await {
            return turns;
        }

        // ── Fallback: DOM sweep (bounded by virtual-list window) ──
        //
        // DeepSeek's virtual-list keeps a bounded render window (~13-33 items).
        // Scrolling unmounts items that leave the viewport. So a single read only
        // captures the current window. To capture the ENTIRE conversation we sweep
        // top→bottom, reading at each position and accumulating by key (dedup).
        //
        // The sweep terminates when the accumulated key-set stops growing for
        // several consecutive steps. scrollTop is NOT a reliable terminator —
        // DeepSeek clamps it to a minimum and adjusts it asynchronously.
        //
        // Read the current virtual-list window — returns JSON array with `key`.
        let read_items_js = r#"
(() => {
    const vl = document.querySelector('.ds-virtual-list');
    if (!vl) return JSON.stringify([]);
    const items = vl.querySelectorAll('[data-virtual-list-item-key]');
    return JSON.stringify(Array.from(items).map(item => {
        const key = item.getAttribute('data-virtual-list-item-key');
        const msg = item.querySelector('.ds-message');
        if (!msg) return { key, role: 'unknown', content: '', think: '', think_secs: '' };
        const classes = (msg.className || '').split(/\s+/).filter(Boolean);
        const hasHash = classes.some(c => /^d[0-9a-f]{7,}$/i.test(c));
        if (hasHash) {
            return {
                key,
                role: 'user',
                content: ((msg.querySelector('[class*="fbb737a4"]') || msg).textContent || '').trim().substring(0, 20000),
                think: '', think_secs: ''
            };
        } else {
            const main = msg.querySelector('.ds-markdown.ds-assistant-message-main-content');
            let content = main ? main.textContent.trim() : '';
            if (!content) {
                content = Array.from(msg.querySelectorAll('.ds-markdown'))
                    .filter(el => !el.closest('.ds-think-content'))
                    .map(el => el.textContent.trim())
                    .join('\n').trim();
            }
            const thinkEl = msg.querySelector('.ds-think-content');
            let think = '', thinkSecs = '';
            if (thinkEl) {
                think = Array.from(thinkEl.querySelectorAll('.ds-markdown'))
                    .map(md => md.textContent.trim()).join('\n');
                const tm = (msg.textContent || '').match(/已思考[（(]用时\s*(\d+)\s*秒[）)]/);
                if (tm) thinkSecs = tm[1];
            }
            return { key, role: 'assistant', content: content.substring(0,20000), think: think.substring(0,20000), think_secs: thinkSecs };
        }
    }));
})()
"#;

        // Scroll down by a SMALL step. A full viewport-height jump can skip past
        // items the virtual list hasn't mounted yet (it needs a scroll event + a
        // render cycle to mount the next batch). ~30% of viewport keeps us inside
        // the CDK's buffer. Returns new scrollTop for stall detection.
        let scroll_down_js = r#"
(() => {
    const vl = document.querySelector('.ds-virtual-list');
    if (!vl) return '0,0,0';
    const step = Math.max(150, Math.floor(vl.clientHeight * 0.3));
    vl.scrollTop = Math.min(vl.scrollTop + step, vl.scrollHeight);
    vl.dispatchEvent(new Event('scroll', {bubbles: true}));
    return Math.round(vl.scrollTop) + ',' + Math.round(vl.scrollHeight) + ',' + Math.round(vl.clientHeight);
})()
"#;

        // Scroll to top first (DeepSeek may clamp, but we try).
        self.kimi
            .eval_js(
                r#"(() => { const vl=document.querySelector('.ds-virtual-list'); if(vl) { vl.scrollTop=0; vl.dispatchEvent(new Event('scroll',{bubbles:true})); } return ''; })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        let mut accumulated: BTreeMap<u64, serde_json::Value> = BTreeMap::new();
        let mut last_scroll_top: i64 = -1;
        let mut stuck_count = 0u32; // consecutive steps with no new keys
        let mut stall_count = 0u32; // consecutive steps where scrollTop didn't move

        for _step in 0..500 {
            let data = self.kimi.eval_json(read_items_js).await;
            let prev_len = accumulated.len();
            if let Some(arr) = data.and_then(|v| v.as_array().map(|a| a.clone())) {
                for item in arr {
                    if let Some(key_str) = item.get("key").and_then(|k| k.as_str()) {
                        if let Ok(key) = key_str.parse::<u64>() {
                            accumulated.entry(key).or_insert(item);
                        }
                    }
                }
            }

            // Termination: no new keys discovered for several steps → swept past end.
            if accumulated.len() == prev_len {
                if !accumulated.is_empty() {
                    stuck_count += 1;
                    if stuck_count >= 8 {
                        break;
                    }
                }
            } else {
                stuck_count = 0;
            }

            // Scroll down by a small step.
            let (scroll_info, _) = self.kimi.eval_js(scroll_down_js).await;
            let parts: Vec<&str> = scroll_info.split(',').collect();
            let new_top: i64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);

            // Detect a stuck scroll. Nudge back to top to re-trigger lazy loading.
            if new_top == last_scroll_top {
                stall_count += 1;
                if stall_count >= 3 {
                    break;
                }
                self.kimi
                    .eval_js(
                        r#"(() => { const vl=document.querySelector('.ds-virtual-list'); if(vl) { vl.scrollTop=0; vl.dispatchEvent(new Event('scroll',{bubbles:true})); } return ''; })()"#,
                    )
                    .await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            } else {
                stall_count = 0;
            }
            last_scroll_top = new_top;
            tokio::time::sleep(Duration::from_millis(30)).await;
        }

        debug!(
            total_keys = accumulated.len(),
            "extract_turns sweep complete"
        );

        let raw_msgs: Vec<serde_json::Value> = accumulated.into_values().collect();

        // Pair consecutive user→assistant messages into ChatTurn
        let mut turns: Vec<ChatTurn> = Vec::new();
        let mut i = 0;
        while i < raw_msgs.len() {
            let role = raw_msgs[i]
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = raw_msgs[i]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if role == "user" {
                // Check if next message is assistant
                if i + 1 < raw_msgs.len() {
                    let next_role = raw_msgs[i + 1]
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if next_role == "assistant" {
                        let resp = raw_msgs[i + 1]
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let think = extract_thinking_trace(&raw_msgs[i + 1]);
                        turns.push(ChatTurn {
                            user_message: content,
                            assistant_response: resp,
                            thinking_trace: think,
                            timestamp: 0.0,
                        });
                        i += 2;
                        continue;
                    }
                }
                // Unpaired user message (no following assistant)
                turns.push(ChatTurn {
                    user_message: content,
                    assistant_response: String::new(),
                    thinking_trace: None,
                    timestamp: 0.0,
                });
                i += 1;
            } else {
                // Orphan assistant message (shouldn't happen, be defensive)
                turns.push(ChatTurn {
                    user_message: String::new(),
                    assistant_response: content,
                    thinking_trace: None,
                    timestamp: 0.0,
                });
                i += 1;
            }
        }

        debug!(turn_count = turns.len(), "extract_turns done");
        turns
    }

    /// Fetch the full conversation via DeepSeek's history API (fast path).
    ///
    /// Calls `GET /api/v0/chat/history_messages?chat_session_id=<id>` with the
    /// `userToken` from localStorage. Returns the full message list in one shot —
    /// no virtual-list truncation. Returns `None` if the API is unavailable (no
    /// session id in URL, not logged in, or request fails).
    async fn extract_turns_via_api(&self) -> Option<Vec<ChatTurn>> {
        let data = self
            .kimi
            .eval_json(
                r#"(async () => {
            // Extract session id from URL: /a/chat/s/<uuid>
            const m = window.location.pathname.match(/\/a\/chat\/s\/([0-9a-f-]+)/i);
            if (!m) return null;
            const sessionId = m[1];

            // Read auth token from localStorage
            const rawToken = localStorage.getItem('userToken');
            if (!rawToken) return null;
            let token;
            try { token = JSON.parse(rawToken).value; } catch(e) { return null; }
            if (!token) return null;

            try {
                const r = await fetch('/api/v0/chat/history_messages?chat_session_id=' + sessionId, {
                    credentials: 'include',
                    headers: { 'Authorization': token }
                });
                if (!r.ok) return null;
                const parsed = await r.json();
                if (parsed.code !== 0 || !parsed.data || !parsed.data.biz_data) return null;
                const msgs = parsed.data.biz_data.chat_messages || [];
                // Return a compact array — only the fields we need for pairing.
                return JSON.stringify(msgs.map(m => ({
                    role: (m.role || '').toUpperCase() === 'USER' ? 'user' : 'assistant',
                    content: (m.content || '').substring(0, 20000),
                    think: (m.thinking_content || '').substring(0, 20000),
                    think_secs: String(m.thinking_elapsed_secs || '')
                })));
            } catch(e) {
                return null;
            }
        })()"#,
            )
            .await;

        let arr = data?;
        let msgs = arr.as_array()?;
        if msgs.is_empty() {
            return None;
        }

        // Pair consecutive user→assistant (same logic as the DOM fallback).
        let mut turns: Vec<ChatTurn> = Vec::new();
        let mut i = 0;
        while i < msgs.len() {
            let role = msgs[i].get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content = msgs[i]
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if role == "user" {
                if i + 1 < msgs.len() {
                    let next_role = msgs[i + 1]
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if next_role == "assistant" {
                        let resp = msgs[i + 1]
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        turns.push(ChatTurn {
                            user_message: content,
                            assistant_response: resp,
                            thinking_trace: extract_thinking_trace(&msgs[i + 1]),
                            timestamp: 0.0,
                        });
                        i += 2;
                        continue;
                    }
                }
                turns.push(ChatTurn {
                    user_message: content,
                    assistant_response: String::new(),
                    thinking_trace: None,
                    timestamp: 0.0,
                });
                i += 1;
            } else {
                turns.push(ChatTurn {
                    user_message: String::new(),
                    assistant_response: content,
                    thinking_trace: extract_thinking_trace(&msgs[i]),
                    timestamp: 0.0,
                });
                i += 1;
            }
        }
        debug!(
            turn_count = turns.len(),
            source = "api",
            "extract_turns via API"
        );
        Some(turns)
    }

    // ── Session navigation ──

    /// Open a historical conversation by session URL or sidebar title.
    ///
    /// If `query` contains "/a/chat/s/", treat it as a URL and navigate directly.
    /// Otherwise, fuzzy-match it against the sidebar history titles (a[href*="/a/chat/s/"])
    /// and click the first match. Waits for the virtual list to re-hydrate before returning.
    pub async fn open_session(&self, query: &str) -> Result<()> {
        if query.contains("/a/chat/s/") {
            let url = if query.starts_with('/') {
                format!("https://chat.deepseek.com{}", query)
            } else {
                query.to_string()
            };
            debug!(url = %url, "navigating to session URL");
            self.kimi.navigate(&url, false).await?;
        } else {
            let query_lower = query.to_lowercase();
            let safe = serde_json::to_string(&query_lower)?;
            let (found, _) = self
                .kimi
                .eval_js(&format!(
                    r#"(() => {{
                    const links = document.querySelectorAll('a[href*="/a/chat/s/"]');
                    for (const a of links) {{
                        if (a.textContent.trim().toLowerCase().includes({})) {{
                            a.click();
                            return true;
                        }}
                    }}
                    return false;
                }})()"#,
                    safe
                ))
                .await;
            if found != "true" {
                return Err(AdapterError::ElementNotFound {
                    selector: format!("history item matching '{}'", query),
                });
            }
            debug!(query = %query, "clicked sidebar history item");
        }

        // VERIFY 1: URL should now contain /a/chat/s/
        let mut url_ok = false;
        for _ in 0..30 {
            let url = self.kimi.get_url().await;
            if url.contains("/a/chat/s/") {
                url_ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if !url_ok {
            warn!("open_session: URL never settled to /a/chat/s/");
            return Err(AdapterError::PageNotReady {
                reason: "session URL did not settle".into(),
            });
        }

        // VERIFY 2: wait for virtual list to hydrate
        for _ in 0..20 {
            let (ready, _) = self
                .kimi
                .eval_js(
                    r#"(() => {
                    const vl = document.querySelector('.ds-virtual-list');
                    const msgs = document.querySelectorAll('.ds-message').length;
                    const ta = !!document.querySelector('textarea');
                    if (vl && msgs > 0) return 'ready';
                    if (ta) return 'ready';
                    return 'waiting';
                })()"#,
                )
                .await;
            if ready == "ready" {
                debug!("open_session: session hydrated");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        warn!("open_session: session did not hydrate within timeout");
        Err(AdapterError::PageNotReady {
            reason: "session did not hydrate".into(),
        })
    }

    /// Full send pipeline: send → wait for streaming to settle → extract with
    /// stability confirmation. This is the production-grade send path for
    /// DeepSeek's virtual-list + streaming-render combination, which the bare
    /// `send_message` does not handle.
    ///
    /// - Captures a baseline so we never return a stale/duplicate response.
    /// - Fast-fails on service errors (rate limit / server busy) without
    ///   wasting the retry budget.
    /// - Confirms content is stable across two reads before returning.
    pub async fn send_and_wait(&self, msg: &str) -> Result<String> {
        self.ensure_tab().await?;

        // Capture baseline content (to reject stale/duplicate extraction)
        let baseline = self.extract_last_response().await;

        self.send_message(msg).await?;

        if !self.wait_for_response(Duration::from_secs(60)).await {
            return Err(AdapterError::NoResponse);
        }

        // Fast-fail: if the page already shows a service error, don't retry
        if let Some(err) = self.check_service_error().await {
            warn!(error = %err, "service error after streaming stopped");
            return Err(AdapterError::NoResponse);
        }

        // Extract with backoff + duplicate guard + stability check
        let mut retry_interval = 0.2_f64;
        for i in 0..20 {
            if let Some(err) = self.check_service_error().await {
                warn!(error = %err, "service error detected, aborting extract");
                return Err(AdapterError::NoResponse);
            }

            tokio::time::sleep(Duration::from_secs_f64(retry_interval)).await;
            self.scroll_virtual_list().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let content = self.extract_last_response().await;
            if !content.is_empty() && content != baseline {
                // Stability: re-read after cooldown, must match
                tokio::time::sleep(Duration::from_millis(500)).await;
                self.scroll_virtual_list().await;
                tokio::time::sleep(Duration::from_millis(200)).await;
                let confirm = self.extract_last_response().await;
                if confirm == content {
                    info!(len = content.len(), retry = i, "send_and_wait ok");
                    return Ok(content);
                }
                debug!(retry = i, "content unstable, retrying");
            }
            retry_interval = (retry_interval * 1.5).min(2.0);
        }

        warn!("no stable response after 20 fallbacks");
        Err(AdapterError::NoResponse)
    }
}

impl From<KimiPrimitives> for DeepSeekSemantics {
    fn from(kimi: KimiPrimitives) -> Self {
        Self::new(kimi)
    }
}

/// Extract ThinkingTrace from a raw JSON message object.
fn extract_thinking_trace(v: &serde_json::Value) -> Option<ThinkingTrace> {
    let think = v.get("think").and_then(|s| s.as_str()).unwrap_or("");
    if think.is_empty() {
        return None;
    }
    let time = v
        .get("think_secs")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Some(ThinkingTrace {
        content: think.to_string(),
        time,
    })
}
