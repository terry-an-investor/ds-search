//! AistudioSemantics — Google AI Studio operations.
//!
//! Features: send prompts (observe→act→verify hardened), extract responses,
// select models, set thinking level, browse history, extract full
// conversations, system instructions, tool toggles, temperature,
// thinking/reasoning extraction, and lightweight page-state inspection.
//
// Selectors are documented in knowledge/aistudio.google.com.yaml. Where the
// DOM cannot be verified live, extraction uses multiple defensive strategies
// (deepseek::extract_last_response pattern) and never panics — it returns
// empty strings / None / default structs instead.

use pilot::error::{AdapterError, Result};
use pilot::kimi::KimiPrimitives;
use std::time::Duration;
use tracing::debug;

use crate::models::{AistudioModel, AistudioState, ChatTurn, Conversation, Tool, TurnRole};

const PLAYGROUND_URL: &str = "https://aistudio.google.com/prompts/new_chat";
const HISTORY_URL: &str = "https://aistudio.google.com/library";

#[derive(Debug, Clone)]
pub struct AistudioSemantics {
    pub kimi: KimiPrimitives,
}

// One line: opt into the verify-driven primitives. After this, `self` can call
// `self.fill_and_verify(...)` / `self.act_and_verify(...)` from the VerifyDriven
// trait, and a failed VERIFY returns AdapterError::VerifyFailed with a diff
// instead of a silent Ok(()).
impl pilot::verify::KimiRef for AistudioSemantics {
    fn kimi_ref(&self) -> &KimiPrimitives {
        &self.kimi
    }
}

impl AistudioSemantics {
    pub fn new(kimi: KimiPrimitives) -> Self {
        Self { kimi }
    }

    // ═══════════════════════════════════════════════════════════
    // Tab management
    // ═══════════════════════════════════════════════════════════

    /// Ensure we have an AI Studio tab open and the playground has hydrated
    /// (textarea present), with no blocking dialog overlays. Mirrors
    /// gemini/deepseek ensure_tab — without the hydration wait, a send right
    /// after navigation lands on a half-loaded page and the prompt silently
    /// drops. `dismiss_dialogs` runs first because leftover overlays (e.g. the
    /// "Get code" modal) intercept clicks and silently break every subsequent
    /// interaction — this is the #1 cause of "prompt not accepted" failures.
    pub async fn ensure_tab(&self) -> Result<()> {
        let url = self.kimi.get_url().await;
        if !url.contains("aistudio.google.com") {
            self.kimi.navigate(PLAYGROUND_URL, false).await?;
        }
        self.dismiss_dialogs().await;
        // Wait for the prompt textarea to render (SPA hydration).
        for _ in 0..20 {
            let (has, _) = self
                .kimi
                .eval_js("!!document.querySelector('textarea')")
                .await;
            if has == "true" {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Dismiss every Angular CDK overlay / modal dialog on the page.
    ///
    /// AI Studio surfaces "Get code", model-picker, and tool-config as CDK
    /// overlays (`.cdk-overlay-pane`). When one is open it sits above the
    /// playground and swallows clicks, so a `send_prompt` / `click_run` silently
    /// no-ops. Strategy: (1) click each visible pane's close button; (2) re-check
    /// for *still-visible* panes (covers the no-close-button case, e.g. a bare-mask
    /// modal) and dispatch Escape on those — but only on a real dialog element,
    /// never on the active element (which may be the textarea; Escape there wipes
    /// its value — that was an earlier bug). Escape is NEVER sent unconditionally:
    /// if no pane is visible, we do nothing.
    pub async fn dismiss_dialogs(&self) {
        // Pass 1: click close buttons on every visible pane.
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                document.querySelectorAll('.cdk-overlay-pane, [role="dialog"]').forEach(p => {
                    if (window.getComputedStyle(p).display === 'none' || p.offsetParent === null) return;
                    const btn = p.querySelector('button[aria-label="Close"], button[aria-label="close"], .mat-mdc-dialog-close')
                        || Array.from(p.querySelectorAll('button')).find(b => (b.textContent || '').trim() === 'Close');
                    if (btn) btn.click();
                });
                return 'ok';
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Pass 2: any pane STILL visible after the button pass? (No-close-button
        // modals land here.) Dispatch Escape on it — but guard against the
        // textarea: if the still-visible "pane" is actually the textarea or body,
        // skip (those aren't dialogs). This is what keeps Escape from wiping the
        // prompt input when the page is clean.
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                const panes = Array.from(document.querySelectorAll('.cdk-overlay-pane, [role="dialog"]'))
                    .filter(p => p.offsetParent !== null && window.getComputedStyle(p).display !== 'none');
                if (!panes.length) return 'no_dialog';
                // Escape should target the overlay container (Angular CDK listens
                // there), not the textarea. Dispatch on each pane + the container.
                const target = document.querySelector('.cdk-overlay-container') || panes[0];
                [target, ...panes].forEach(el => {
                    el.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', keyCode: 27, bubbles: true}));
                });
                return 'escaped';
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
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

    // NOTE: prompt filling is done via the `VerifyDriven::fill_and_verify`
    // trait method (see send_prompt) — it fills AND verifies the value landed
    // in one call, replacing the old `set_prompt_text` that returned Ok without
    // ever reading the textarea back.

    /// Click the Run button.
    async fn click_run(&self) -> Result<()> {
        let (found, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.textContent.includes('Run') && !b.disabled);
                if (btn) { btn.click(); return 'ok'; }
                return 'not_found';
            })()"#,
            )
            .await;
        if found.contains("not_found") {
            return Err(AdapterError::ElementNotFound {
                selector: "Run button".into(),
            });
        }
        Ok(())
    }

    /// Send a prompt by setting the textarea and clicking Run.
    ///
    /// Hardened per AGENTS.md observe→act→verify: a successful send must
    /// produce a *new* user turn on the page (Angular renders `.chat-turn-container.user`
    /// when the prompt is accepted). If the click appears to succeed but no new
    /// turn appears, we fall back to a second Run click, then to dispatching an
    /// Enter keydown on the textarea. Returning `Ok(())` only means the prompt
    /// was *dispatched* and accepted — call `wait_for_response` to wait for the
    /// model reply.
    pub async fn send_prompt(&self, text: &str) -> Result<()> {
        self.ensure_tab().await?;

        // ── OBSERVE: baseline state ──
        let before_count = self.user_turn_count().await;

        // ── ACT + VERIFY: fill the PROMPT box (not the system-instructions box),
        // and prove the text landed before clicking Run.  The page has TWO
        // textareas; the first is system instructions, the second is the actual
        // prompt input.  DomState::textarea_value is HARDCODED to the first
        // textarea (see verify.rs line 99), so the VERIFY reads the precise
        // prompt box via extra_js — matching the fill target exactly.
        use pilot::verify::VerifyDriven;
        let prompt_sel = r#"textarea[placeholder*="Start typing"]"#;
        let extra_js = concat!(
            r#"(()=>{const ta=document.querySelector('textarea[placeholder*="Start typing"]');"#,
            r#"return ta?(ta.value||''):'';})()"#,
        );
        if self
            .fill_and_verify(prompt_sel, text, Some(extra_js), |after| {
                after.extra.as_str().map(|s| s == text).unwrap_or(false)
            })
            .await
            .is_err()
        {
            // Fallback: retry once (the form may need focus first).
            self.fill_and_verify(prompt_sel, text, Some(extra_js), |after| {
                after.extra.as_str().map(|s| s == text).unwrap_or(false)
            })
            .await?;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;

        // ── ACT: click Run ──
        self.click_run().await?;

        // ── VERIFY: a new user turn must appear ──
        tokio::time::sleep(Duration::from_millis(700)).await;
        if self.user_turn_count().await > before_count {
            debug!(msg_len = text.len(), "prompt accepted (new user turn)");
            return Ok(());
        }

        // Fallback 1: Run may have needed the textarea to be re-acknowledged.
        debug!("no new user turn after first click, retrying Run");
        self.click_run().await?;
        tokio::time::sleep(Duration::from_millis(700)).await;
        if self.user_turn_count().await > before_count {
            debug!(msg_len = text.len(), "prompt accepted on Run retry");
            return Ok(());
        }

        // Fallback 2: dispatch Enter on the PROMPT textarea (not the system-
        // instructions box). Using the precise selector matches the fill step.
        debug!("no new user turn after Run retry, dispatching Enter");
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                const ta = document.querySelector('textarea[placeholder*="Start typing"]');
                if (!ta) return;
                ta.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
                    bubbles: true, cancelable: true
                }));
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(700)).await;
        if self.user_turn_count().await > before_count {
            debug!(msg_len = text.len(), "prompt accepted via Enter");
            return Ok(());
        }

        // All fallbacks exhausted — surface the failure rather than continue.
        Err(AdapterError::SendFailed {
            reason: "prompt not accepted: no new user turn after Run/Enter".into(),
        })
    }

    /// Convenience pipeline: send → wait → extract the latest model reply,
    /// with **failure recovery**. AI Studio generation frequently fails (network
    /// blip, rate limit, model error) leaving a model turn with no run-time pill
    /// or an on-page error. When that happens we click "Rerun this turn" and
    /// re-wait, up to `max_retries` times, before giving up. Mirrors the
    /// real-world usage pattern: send, and if no reply comes back, rerun.
    pub async fn send_and_wait(&self, prompt: &str) -> Result<String> {
        self.send_prompt(prompt).await?;
        self.wait_for_response_or_rerun(3).await
    }

    /// Wait for the latest model turn to finish; if generation failed, click
    /// "Rerun this turn" and wait again, up to `max_retries` times. Returns the
    /// extracted reply text. This is the resilience core — every `ask` /
    /// `send_and_wait` goes through it.
    pub async fn wait_for_response_or_rerun(&self, max_retries: u32) -> Result<String> {
        // NOTE: the loop body always tries to EXTRACT before deciding failure.
        // Earlier versions returned NoResponse on the last iteration as soon as
        // check_generation_error() was Some — but that check can briefly misfire
        // while streaming settles, discarding a recovery that actually succeeded.
        // Now: extract first; only treat as failure if there's truly nothing to
        // return AND an error is confirmed.
        for attempt in 0..=max_retries {
            // Wait up to 60s for the turn to settle. (We don't branch on `ready`
            // directly — extract_response below is the real signal — but a timed-
            // out wait means streaming is likely still going, which informs the
            // empty-extract fallback below rather than triggering a rerun.)
            let _ready = self.wait_for_response(60).await.unwrap_or(false);

            // Always try to extract first — a successful rerun on the previous
            // iteration may already have produced a pill + content.
            let resp = self.extract_response().await;
            if !resp.is_empty() {
                return Ok(resp);
            }

            // Nothing extracted yet. Is it a real failure, or just the turn not
            // ready / virtual scroll not mounted?
            let errored = self.check_generation_error().await;
            if let Some(err) = errored {
                debug!(attempt = attempt, error = %err, "generation failed, will rerun");
                if attempt < max_retries {
                    self.rerun_last_turn().await?;
                    continue;
                }
                // Last attempt: the error is confirmed and there's no content.
                return Err(AdapterError::NoResponse);
            }

            // No error detected, but also no content. Likely the turn is still
            // streaming (ready=false) or virtual scroll hasn't mounted. Retry the
            // read rather than rerunning (rerun would discard an in-flight reply).
            if attempt < max_retries {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        }
        Err(AdapterError::NoResponse)
    }

    /// Count `.chat-turn-container.user` elements currently rendered.
    pub async fn user_turn_count(&self) -> usize {
        let (s, _) = self
            .kimi
            .eval_js("document.querySelectorAll('.chat-turn-container.user').length")
            .await;
        s.trim().parse().unwrap_or(0)
    }

    /// Wait for a response to complete. Polls until the run-time pill appears
    /// (the authoritative "generation finished" signal) or — as a fallback —
    /// until the model-turn text length is stable for 3 rounds.
    pub async fn wait_for_response(&self, timeout_secs: u64) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut last_len = 0usize;
        let mut stable = 0u32;

        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;
            // Primary signal: the run-time pill only renders after generation.
            // Validate on the Rust side too: the mock bridge returns the raw value
            // without running the browser regex, and a real page can surface stray text.
            let (pill, _) = self
                .kimi
                .eval_js(
                    "(() => { const p = document.querySelector('.model-run-time-pill'); return p ? (p.textContent || '').trim() : ''; })()",
                )
                .await;
            let pill = pill.trim();
            // A run-time pill looks like "3.7s" — must end in 's' and contain a digit.
            if pill.ends_with('s')
                && pill.chars().any(|c| c.is_ascii_digit())
                && pill
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == 's')
            {
                return Ok(true);
            }
            // Fallback: model-turn text length stability.
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

    // ═══════════════════════════════════════════════════════════
    // Generation resilience (rerun on failure)
    // ═══════════════════════════════════════════════════════════

    /// Detect whether the latest model turn failed to generate.
    ///
    /// A successful generation ends with a run-time pill (`.model-run-time-pill`,
    /// e.g. "3.7s") and no error text. A failed generation leaves the turn with
    /// NO pill after streaming stopped (the page shows a "Try again"/"Error"/
    /// "Something went wrong" affordance instead), or shows an explicit error
    /// string in the model turn's container. Returns `Some(reason)` on failure,
    /// `None` on success/unknown. Verified live: a clean turn carries a pill.
    pub async fn check_generation_error(&self) -> Option<String> {
        // First: any explicit error text in the latest model turn.
        let (err, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const turns = document.querySelectorAll('.chat-turn-container.model');
                if (!turns.length) return '';
                const last = turns[turns.length - 1];
                const t = (last.textContent || '').toLowerCase();
                const patterns = ['try again', 'something went wrong', 'an error occurred',
                                  'failed to generate', 'request failed', 'rate limit',
                                  'internal error', 'generation failed'];
                for (const p of patterns) { if (t.includes(p)) return p; }
                return '';
            })()"#,
            )
            .await;
        let err = err.trim();
        if !err.is_empty() {
            return Some(format!("generation error: {}", err));
        }
        // Second: no run-time pill after the turn settled. Only treat as failure
        // when a model turn exists at all (otherwise it's just "no turn yet").
        let (turns_str, _) = self
            .kimi
            .eval_js("document.querySelectorAll('.chat-turn-container.model').length")
            .await;
        let model_turns: usize = turns_str.trim().parse().unwrap_or(0);
        if model_turns == 0 {
            return None; // nothing to rerun yet
        }
        let (pill, _) = self
            .kimi
            .eval_js(
                "(() => { const p = document.querySelector('.model-run-time-pill'); return p ? (p.textContent || '').trim() : ''; })()",
            )
            .await;
        let pill = pill.trim();
        // A valid pill ends in 's' and contains a digit (e.g. "3.7s", "1m3.8s").
        let has_pill = pill.ends_with('s')
            && pill.chars().any(|c| c.is_ascii_digit())
            && pill
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == 's' || c == 'm');
        if !has_pill {
            return Some("no run-time pill after generation settled".into());
        }
        None
    }

    /// Click "Rerun this turn" on the latest model turn to retry generation.
    /// The button is `[name="rerun-button"]` / `[aria-label="Rerun this turn"]`
    /// and is always present on a model turn (verified live).
    pub async fn rerun_last_turn(&self) -> Result<()> {
        self.dismiss_dialogs().await;
        let (result, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                // Prefer the rerun button on the LAST model turn specifically,
                // so we don't accidentally rerun an earlier one.
                const turns = document.querySelectorAll('.chat-turn-container.model');
                if (!turns.length) return 'no_model_turn';
                const last = turns[turns.length - 1];
                last.scrollIntoView();
                const btn = last.querySelector('[name="rerun-button"], [aria-label="Rerun this turn"]')
                    || last.querySelector('button[aria-label*="Rerun" i]');
                if (btn) { btn.click(); return 'rerun'; }
                return 'no_rerun_button';
            })()"#,
            )
            .await;
        match result.as_str() {
            "rerun" => {
                debug!("clicked Rerun this turn");
                Ok(())
            }
            "no_model_turn" => Err(AdapterError::ElementNotFound {
                selector: "model turn to rerun".into(),
            }),
            _ => Err(AdapterError::ElementNotFound {
                selector: "Rerun this turn button".into(),
            }),
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Feedback & sharing
    // ═══════════════════════════════════════════════════════════

    /// Rate the latest model response: `up` (Good response) or `down` (Bad response).
    /// Buttons are `[aria-label="Good response"]` / `[aria-label="Bad response"]`
    /// on the last model turn. Returns true if clicked.
    pub async fn rate_response(&self, up: bool) -> Result<bool> {
        let label = if up { "Good response" } else { "Bad response" };
        let (result, _) = self
            .kimi
            .eval_js(&format!(
                r#"(() => {{
                const turns = document.querySelectorAll('.chat-turn-container.model');
                if (!turns.length) return 'no_model_turn';
                const last = turns[turns.length - 1];
                last.scrollIntoView();
                const btn = last.querySelector('button[aria-label="{}"]');
                if (btn) {{ btn.click(); return 'clicked'; }}
                return 'not_found';
            }})()"#,
                label
            ))
            .await;
        match result.as_str() {
            "clicked" => Ok(true),
            "no_model_turn" => Err(AdapterError::ElementNotFound {
                selector: "model turn to rate".into(),
            }),
            _ => Err(AdapterError::ElementNotFound {
                selector: format!("{} button", label),
            }),
        }
    }

    /// Click the "Share prompt" button (`[aria-label="Share prompt"]`) to open
    /// the share dialog. Caller can then read the share link from the dialog.
    /// Returns true if the button was found and clicked.
    pub async fn share(&self) -> Result<bool> {
        self.dismiss_dialogs().await;
        let (result, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = document.querySelector('button[aria-label="Share prompt"]');
                if (btn) { btn.click(); return 'clicked'; }
                return 'not_found';
            })()"#,
            )
            .await;
        match result.as_str() {
            "clicked" => Ok(true),
            _ => Err(AdapterError::ElementNotFound {
                selector: "Share prompt button".into(),
            }),
        }
    }

    /// Extract the share link from an open Share dialog. AI Studio's share
    /// component mounts as `[role=dialog]` directly under <body> (NOT a
    /// `.cdk-overlay-pane`), and link creation is asynchronous — the dialog
    /// shows "正在加载…" until the backend mints the URL. We look for an input
    /// value or an href inside any open dialog. Returns None if no dialog is
    /// open, the link isn't ready yet, or the share service didn't complete.
    pub async fn get_share_link(&self) -> Option<String> {
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                // The share dialog is a [role=dialog] under <body>, not a cdk overlay.
                const dlgs = document.querySelectorAll('[role=dialog], .cdk-overlay-pane');
                for (const d of dlgs) {
                    const input = d.querySelector('input[type=text], input[type=url], input[readonly]');
                    if (input && input.value && input.value.startsWith('http')) return input.value;
                    const link = d.querySelector('a[href*="aistudio.google.com"]');
                    if (link) return link.href;
                }
                return '';
            })()"#,
            )
            .await;
        let s = raw.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// Extract the text of the latest model response.
    ///
    /// Uses the same structural approach as `extract_turns`: scroll the last
    /// model turn into view (Angular virtualization), then read its `.text-chunk`
    /// elements and join them. This avoids scraping the container's textContent
    /// (which mixes in the `.author-label` "Model 10:39 PM" header and run-time
    /// pill) and is robust to timestamp-format changes. Verified live.
    pub async fn extract_response(&self) -> String {
        let (text, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const turns = document.querySelectorAll('.chat-turn-container.model');
                if (!turns.length) return '';
                const last = turns[turns.length - 1];
                last.scrollIntoView();
                const chunks = Array.from(last.querySelectorAll('.text-chunk'))
                    .map(e => (e.textContent || '').trim())
                    .filter(s => s.length > 0);
                return chunks.join('\n').substring(0, 20000);
            })()"#,
            )
            .await;
        text
    }

    // ═══════════════════════════════════════════════════════════
    // Full conversation extraction
    // ═══════════════════════════════════════════════════════════

    /// Scroll the Angular virtual-scroll container and any message containers
    /// to trigger lazy rendering of turns (AI Studio uses virtual scrolling;
    /// off-screen turns are not in the DOM until scrolled into view).
    pub async fn scroll_chat(&self) {
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                // Primary: Angular virtual-scroll container.
                document.querySelectorAll('.virtual-scroll-container').forEach(el => {
                    try { el.scrollTop = el.scrollHeight; } catch(e) {}
                });
                // Secondary: any scroll area that holds chat turns.
                document.querySelectorAll('*').forEach(el => {
                    try {
                        const cs = window.getComputedStyle(el);
                        if ((cs.overflowY !== 'scroll' && cs.overflowY !== 'auto') ||
                            el.scrollHeight <= el.clientHeight + 5) return;
                        if (!el.querySelector('.chat-turn-container')) return;
                        el.scrollTop = el.scrollHeight;
                    } catch(e) {}
                });
            })()"#,
            )
            .await;
    }

    /// Extract every chat turn (user + model, in order) as typed data.
    ///
    /// AI Studio structures each turn as:
    ///   `.chat-turn-container.{user|model}`
    ///     └ `.virtual-scroll-container` (virtualized; text absent until scrolled in)
    ///         └ `.turn-content`
    ///             ├ `.author-label`   ← "Model 10:39 PM" label (NOISE)
    ///             ├ `MS-PROMPT-CHUNK.text-chunk` × N  ← the actual content
    ///             └ `.turn-information`               ← metadata
    ///
    /// We read the `.text-chunk` elements directly — structural extraction
    /// rather than scraping the container's textContent and stripping the label
    /// via regex. This is robust to timestamp format changes (locale, 12/24h,
    /// narrow-no-break-space, etc.) because we never look at the label at all.
    ///
    /// CAVEAT (verified live): AI Studio's Angular CDK virtual scroll aggressively
    /// unmounts turn content. `scrollIntoView` does NOT always force a remount —
    /// `.turn-content` can stay empty (`<!---->`) even after scrolling, because
    /// the viewport's internal bookkeeping decides a turn is off-screen. There is
    /// no JS-only way to reliably force remount. So we retry a few times with
    /// delays; if a turn still has no `.text-chunk`, we return its index with a
    /// placeholder so callers know which turns are missing rather than getting
    /// silently wrong data. Verified live on aistudio.google.com.
    pub async fn extract_turns(&self) -> Vec<ChatTurn> {
        // Retry the scroll + read a few times: Angular sometimes needs multiple
        // scroll events + a delay before it remounts a turn's content.
        let mut turns = vec![];
        for attempt in 0..4 {
            self.scroll_chat().await;
            turns = self.eval_turns_once().await;
            let all_mounted = !turns.is_empty()
                && turns
                    .iter()
                    .all(|t| !t.content.starts_with("(content not rendered"));
            if all_mounted || attempt + 1 == 4 {
                return turns;
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        turns
    }

    /// Single scroll + read pass used by `extract_turns`. Each turn's `.text-chunk`
    /// children are joined; turns with no chunks get a placeholder.
    async fn eval_turns_once(&self) -> Vec<ChatTurn> {
        let raw = self
            .kimi
            .eval_json(
                r#"JSON.stringify((() => {
                const out = [];
                const turns = document.querySelectorAll('.chat-turn-container');
                turns.forEach(t => {
                    let role = t.classList.contains('user') ? 'user' :
                               (t.classList.contains('model') ? 'model' : 'unknown');
                    if (role === 'unknown') return;
                    // Force Angular to mount this turn's content.
                    t.scrollIntoView();
                    // Read content chunks; join multi-paragraph turns with newlines.
                    const chunks = Array.from(t.querySelectorAll('.text-chunk'))
                        .map(e => (e.textContent || '').trim())
                        .filter(s => s.length > 0);
                    let text = chunks.join('\n');
                    if (!text) text = '(content not rendered — Angular virtualized it out; retry or scroll the page manually)';
                    out.push({role: role, content: text.substring(0, 20000)});
                });
                return out;
            })())"#,
            )
            .await;

        let arr = match raw {
            Some(v) if v.is_array() => v,
            _ => return vec![],
        };
        arr.as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| {
                let role = t.get("role").and_then(|r| r.as_str())?;
                let content = t
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let role = if role == "user" {
                    TurnRole::User
                } else {
                    TurnRole::Model
                };
                Some(ChatTurn { role, content })
            })
            .collect()
    }

    /// Extract the full conversation (title + url + ordered turns).
    pub async fn extract_conversation(&self) -> Conversation {
        let (title, url) = tokio::join!(self.current_title(), self.kimi.get_url());
        let turns = self.extract_turns().await;
        Conversation { title, url, turns }
    }

    /// Plain-text rendering of the whole conversation, one turn per block
    /// (`[user] ...` / `[model] ...`). Falls back to a body-innerText pass if
    /// no `.chat-turn-container` elements are found.
    pub async fn extract_conversation_text(&self) -> String {
        let turns = self.extract_turns().await;
        if !turns.is_empty() {
            return turns
                .into_iter()
                .map(|t| format!("[{}] {}", t.role.as_label(), t.content))
                .collect::<Vec<_>>()
                .join("\n\n");
        }

        // Best-effort fallback: if no turns parsed, segment document.body.innerText
        // by "User"/"Model <time>" markers. (Note: on modern AI Studio the turn
        // text is virtualized out of body.innerText, so this usually returns ""
        // — prefer extract_turns() which scrollIntoViews each turn.)
        let (text, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const body = document.body.innerText;
                const parts = body.split(/\n(?=User\s+\d|Model\s+\d)/).filter(p => p && p.trim());
                return parts.map(p => p.trim()).join('\n\n').substring(0, 30000);
            })()"#,
            )
            .await;
        text
    }

    // ═══════════════════════════════════════════════════════════
    // System instructions
    // ═══════════════════════════════════════════════════════════

    /// Expand the collapsible "System instructions" section if its textarea
    /// isn't already rendered. No-op once expanded. The toggle button is the
    /// one whose text starts with "System instructions" (verified via meta scan).
    async fn expand_system_instructions(&self) {
        let (already, _) = self
            .kimi
            .eval_js("!!document.querySelector('textarea[placeholder*=\"tone and style\"]')")
            .await;
        if already == "true" {
            return;
        }
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => /^System instructions/i.test((b.textContent || '').trim()));
                if (btn) btn.click();
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    /// Read the optional system-instructions text.
    ///
    /// System instructions live behind a collapsible "System instructions" button
    /// (verified via `meta scan`); when collapsed there is no textarea. We expand
    /// it first, then locate the field by its placeholder
    /// ("Optional tone and style instructions for the model") — a structural
    /// selector that survives DOM reordering (the field renders *above* the
    /// prompt input when expanded, so a positional "second textarea" guess is wrong).
    pub async fn get_system_instructions(&self) -> String {
        self.expand_system_instructions().await;
        let (text, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const ta = document.querySelector('textarea[placeholder*="tone and style"], textarea[placeholder*="system" i]');
                return ta ? (ta.value || '').trim() : '';
            })()"#,
            )
            .await;
        text
    }

    /// Set the system-instructions text via the native value setter + events.
    /// Expands the section first (see `get_system_instructions`).
    pub async fn set_system_instructions(&self, text: &str) -> Result<()> {
        self.expand_system_instructions().await;
        let safe = serde_json::to_string(text)?;
        let (result, _) = self
            .kimi
            .eval_js(&format!(
                r#"(() => {{
                const ta = document.querySelector('textarea[placeholder*="tone and style"], textarea[placeholder*="system" i]');
                if (!ta) return 'no_target';
                const pd = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value');
                if (pd && pd.set) pd.set.call(ta, {0});
                ta.dispatchEvent(new Event('input', {{bubbles: true}}));
                ta.dispatchEvent(new Event('change', {{bubbles: true}}));
                return 'ok';
            }})()"#,
                safe
            ))
            .await;
        if result.contains("no_target") {
            return Err(AdapterError::ElementNotFound {
                selector: "system instructions textarea/contenteditable".into(),
            });
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Tool toggles
    // ═══════════════════════════════════════════════════════════

    /// Expand the collapsible "Tools" section. No-op if already expanded.
    /// The expander is the `widgets Tools` button; the expanded panel is
    /// `.tools-dialog`. Verified live: a single click opens it.
    async fn expand_tools_section(&self) {
        // Already expanded? Skip to avoid a stray toggle-off.
        let (open, _) = self
            .kimi
            .eval_js("!!document.querySelector('.tools-dialog')")
            .await;
        if open == "true" {
            return;
        }
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => (b.textContent || '').includes('widgets Tools'));
                if (btn) btn.click();
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    /// Toggle a Playground tool. Tools live in `.tool-item` rows inside the
    /// `.tools-dialog` panel; each has a `mat-slide-toggle` whose inner `button`
    /// is the click target. We click that button and read back the toggle state.
    /// `tool.as_label()` matches the row's label text (e.g. "Grounding with
    /// Google Search"). Returns the post-toggle enabled state. Verified live.
    pub async fn toggle_tool(&self, tool: Tool) -> Result<bool> {
        self.expand_tools_section().await;
        let label = tool.as_label().replace('\'', "\\'");
        let (result, _) = self
            .kimi
            .eval_js(&format!(
                r#"(() => {{
                const item = Array.from(document.querySelectorAll('.tool-item'))
                    .find(t => (t.textContent || '').includes('{}'));
                if (!item) return 'not_found';
                const btn = item.querySelector('mat-slide-toggle button');
                if (!btn) return 'no_toggle';
                btn.click();
                return 'clicked';
            }})()"#,
                label
            ))
            .await;
        match result.as_str() {
            "clicked" => Ok(self.is_tool_enabled(tool).await),
            "not_found" => Err(AdapterError::ElementNotFound {
                selector: format!("tool item '{}'", tool.as_label()),
            }),
            _ => Err(AdapterError::ElementNotFound {
                selector: format!("toggle for tool '{}'", tool.as_label()),
            }),
        }
    }

    /// Read whether a tool is enabled. The mat-slide-toggle exposes its state via
    /// the `.mdc-switch--checked` class on the switch, or `aria-checked` on the
    /// button. Verified live.
    pub async fn is_tool_enabled(&self, tool: Tool) -> bool {
        let label = tool.as_label().replace('\'', "\\'");
        let (result, _) = self
            .kimi
            .eval_js(&format!(
                r#"(() => {{
                const item = Array.from(document.querySelectorAll('.tool-item'))
                    .find(t => (t.textContent || '').includes('{}'));
                if (!item) return 'false';
                const sw = item.querySelector('.mdc-switch, [role="switch"]');
                if (!sw) {{
                    const btn = item.querySelector('mat-slide-toggle button');
                    return btn && btn.getAttribute('aria-checked') === 'true' ? 'true' : 'false';
                }}
                if (sw.classList.contains('mdc-switch--checked')) return 'true';
                if (sw.getAttribute('aria-checked') === 'true') return 'true';
                return 'false';
            }})()"#,
                label
            ))
            .await;
        result == "true"
    }

    // ═══════════════════════════════════════════════════════════
    // Temperature
    // ═══════════════════════════════════════════════════════════

    /// Read the current temperature from the slider/input. Returns None if not
    /// found or not parseable.
    pub async fn get_temperature(&self) -> Option<f64> {
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                // Strategy 1: a native range input.
                const range = document.querySelector('input[type="range"]');
                if (range) return range.value;
                // Strategy 2: mat-slider with aria-valuenow.
                const ms = document.querySelector('mat-slider, [role="slider"]');
                if (ms) {
                    const v = ms.getAttribute('aria-valuenow');
                    if (v) return v;
                }
                return '';
            })()"#,
            )
            .await;
        raw.trim().parse::<f64>().ok()
    }

    /// Set the temperature. NOTE: best-effort — Angular Material sliders may
    /// require drag interaction; setting the underlying input value + dispatching
    /// input/change events works for native range inputs. Returns an error only
    /// if no slider element is found at all.
    pub async fn set_temperature(&self, value: f64) -> Result<()> {
        let v = serde_json::to_string(&value)?;
        let (result, _) = self
            .kimi
            .eval_js(&format!(
                r#"(() => {{
                const range = document.querySelector('input[type="range"]');
                if (range) {{
                    const pd = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
                    if (pd && pd.set) pd.set.call(range, {0});
                    range.dispatchEvent(new Event('input', {{bubbles: true}}));
                    range.dispatchEvent(new Event('change', {{bubbles: true}}));
                    return 'ok';
                }}
                const ms = document.querySelector('mat-slider [role="slider"]') ||
                           document.querySelector('[role="slider"]');
                if (ms) {{
                    ms.setAttribute('aria-valuenow', String({0}));
                    ms.dispatchEvent(new Event('input', {{bubbles: true}}));
                    ms.dispatchEvent(new Event('change', {{bubbles: true}}));
                    return 'ok';
                }}
                return 'no_slider';
            }})()"#,
                v
            ))
            .await;
        if result.contains("no_slider") {
            return Err(AdapterError::ElementNotFound {
                selector: "temperature slider".into(),
            });
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Thinking / reasoning extraction
    // ═══════════════════════════════════════════════════════════

    /// Extract the reasoning ("thoughts") content of the latest model turn,
    /// if the model surfaced any. Expands the "Show thinking" affordance first
    /// (like gemini::extract_thinking).
    pub async fn extract_thinking(&self) -> Option<String> {
        // Expand the latest thoughts panel if collapsed.
        let _ = self
            .kimi
            .eval_js(
                r#"(() => {
                const panels = document.querySelectorAll('.model-thoughts, [class*="thoughts"]');
                if (!panels.length) return;
                const last = panels[panels.length - 1];
                const btn = last.querySelector('button, [role="button"]');
                if (btn && /show thinking/i.test(last.textContent || '')) btn.click();
            })()"#,
            )
            .await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const cs = document.querySelectorAll('.thoughts-content, .model-thoughts-content, [class*="thoughts-content"]');
                if (!cs.length) return '';
                const last = cs[cs.length - 1];
                let text = (last.textContent || '').trim();
                text = text.replace(/^Show thinking\s*/i, '');
                return text.substring(0, 10000);
            })()"#,
            )
            .await;
        let text = raw.trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    }

    // ═══════════════════════════════════════════════════════════
    // Page state & streaming
    // ═══════════════════════════════════════════════════════════

    /// Single-eval snapshot of the playground page. Safe for tight polling.
    pub async fn get_state(&self) -> AistudioState {
        let data = self
            .kimi
            .eval_json(
                r#"JSON.stringify((() => {
                const ta = document.querySelector('textarea');
                // A turn is streaming ⟹ there is an active progress indicator.
                // We must EXCLUDE the passive `.loading-token-count-placeholder`,
                // which remains in the DOM after generation completes.
                const spinner = document.querySelector(
                    '.mat-progress-spinner, [role="progressbar"], [aria-busy="true"]'
                );
                const card = document.querySelector('.model-selector-card');
                let model = '';
                if (card) {
                    // Prefer the .title span (name) over the card's full text, which
                    // also contains the model ID and a marketing blurb.
                    const t = card.querySelector('.title, [class*="model-title"]');
                    model = t ? (t.textContent || '').trim() : (card.textContent || '').trim().split('\n')[0];
                }
                const href = window.location.href;
                return {
                    url: href,
                    is_on_playground: href.indexOf('/prompts/') !== -1,
                    has_input: !!ta,
                    is_streaming: !!spinner,
                    user_turn_count: document.querySelectorAll('.chat-turn-container.user').length,
                    model_turn_count: document.querySelectorAll('.chat-turn-container.model').length,
                    current_model: model
                };
            })())"#,
            )
            .await;

        match data {
            Some(v) => AistudioState {
                url: v
                    .get("url")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                is_on_playground: v
                    .get("is_on_playground")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                has_input: v
                    .get("has_input")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                is_streaming: v
                    .get("is_streaming")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                user_turn_count: v
                    .get("user_turn_count")
                    .and_then(|n| n.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(0),
                model_turn_count: v
                    .get("model_turn_count")
                    .and_then(|n| n.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(0),
                current_model: v
                    .get("current_model")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
            },
            None => AistudioState::default(),
        }
    }

    /// Live partial-response snapshot during streaming, as a JSON string.
    /// Shape: `{"processing": bool, "response": str, "thinking": str}`.
    pub async fn get_streaming_state(&self) -> String {
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"JSON.stringify((() => {
                // Exclude the passive token-count placeholder (see get_state).
                const processing = !!document.querySelector(
                    '.mat-progress-spinner, [role="progressbar"], [aria-busy="true"]'
                );
                const turns = document.querySelectorAll('.chat-turn-container.model');
                const resp = turns.length ? (turns[turns.length - 1].textContent || '').trim().substring(0, 2000) : '';
                const ths = document.querySelectorAll('.thoughts-content, [class*="thoughts-content"]');
                const thinking = ths.length ? (ths[ths.length - 1].textContent || '').trim().replace(/^Show thinking\s*/i, '').substring(0, 2000) : '';
                return {processing: processing, response: resp, thinking: thinking};
            })())"#,
            )
            .await;
        raw
    }

    /// Runtime pill for the latest model turn, e.g. "3.9s".
    pub async fn last_response_runtime(&self) -> Option<String> {
        let (raw, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const pill = document.querySelector('.model-run-time-pill, [class*="run-time"]');
                if (!pill) return '';
                return (pill.textContent || '').trim();
            })()"#,
            )
            .await;
        let s = raw.trim().to_string();
        // Validate on the Rust side too: the mock bridge can't run the browser
        // regex, and even a real page can surface stray text near the pill.
        if s.is_empty()
            || !s
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == 's')
        {
            None
        } else {
            Some(s)
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Model selection
    // ═══════════════════════════════════════════════════════════

    /// Get the currently selected model name from the settings panel.
    pub async fn current_model(&self) -> String {
        let (text, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const card = document.querySelector('.model-selector-card');
                if (!card) return '(unknown)';
                // The name lives in a .title span; the ID and blurb are .subtitle siblings.
                const title = card.querySelector('.title, [class*=model-title]');
                if (title) return title.textContent.trim();
                // Fallback: first leaf text run, with the model ID stripped off.
                let raw = (card.textContent || '').trim();
                const idIdx = raw.search(/gemini-[a-z0-9.-]+/);
                return idIdx > 0 ? raw.substring(0, idIdx).trim() : raw;
            })()"#,
            )
            .await;
        text
    }

    /// Open the model selection dialog.
    pub async fn open_model_selector(&self) -> Result<()> {
        let (result, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const card = document.querySelector('.model-selector-card');
                if (!card) return 'no_selector';
                card.click();
                return 'ok';
            })()"#,
            )
            .await;
        if result.contains("no_selector") {
            return Err(AdapterError::ElementNotFound {
                selector: ".model-selector-card".into(),
            });
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
            "no_dialog" => Err(AdapterError::PageNotReady {
                reason: "model dialog not open".into(),
            }),
            _ => Err(AdapterError::SendFailed {
                reason: format!("model '{}' not found in dialog", model_id),
            }),
        }
    }

    /// Set the thinking level (Low/Medium/High).
    ///
    /// OBSERVE → ACT → VERIFY:
    /// 1. Target the `mat-select[aria-label="Thinking Level"]` control (not the
    ///    prompt-template selector that comes first in the DOM).
    /// 2. Open it, click the matching `mat-option`.
    /// 3. Re-read `.mat-mdc-select-min-line` from the Thinking Level select and
    ///    confirm the value was actually applied.
    pub async fn set_thinking_level(&self, level: &str) -> Result<()> {
        let escaped = level.replace('\\', "\\\\").replace('\'', "\\'");
        let code = format!(
            r#"(() => {{
                const sel = document.querySelector('mat-select[aria-label="Thinking Level"]');
                if (!sel) return 'no_select';
                sel.click();
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
            return Err(AdapterError::ElementNotFound {
                selector: "mat-select[aria-label=\"Thinking Level\"]".into(),
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        // ── VERIFY ──────────────────────────────────────────────────────────
        // Re-read the Thinking Level select's current value from the
        // `.mat-mdc-select-min-line` display element (per AGENTS.md the
        // correct VERIFY probe for a mat-select control).
        let verify_code = r#"(() => {
            const sel = document.querySelector('mat-select[aria-label="Thinking Level"]');
            if (!sel) return '';
            const line = sel.querySelector('.mat-mdc-select-min-line');
            return line ? line.textContent.trim() : '';
        })()"#;
        let (actual, _) = self.kimi.eval_js(verify_code).await;
        let actual = actual.trim();
        if actual != level {
            return Err(AdapterError::VerifyFailed {
                action: "set_thinking_level".into(),
                reason: format!("expected Thinking Level '{}', got '{}'", level, actual),
            });
        }
        debug!("Thinking Level set to '{}' (verified)", level);
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════
    // Chat management
    // ═══════════════════════════════════════════════════════════

    /// Start a new chat by clicking the New chat button, then wait for the
    /// playground to settle (URL back to /new_chat + textarea re-rendered).
    pub async fn new_chat(&self) -> Result<()> {
        self.ensure_tab().await?;
        let (result, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.getAttribute('aria-label') === 'New chat');
                if (btn) { btn.click(); return 'ok'; }
                return 'not_found';
            })()"#,
            )
            .await;
        if result.contains("not_found") {
            return Err(AdapterError::ElementNotFound {
                selector: "New chat button".into(),
            });
        }
        // Wait for the SPA to reset: URL returns to /new_chat and turns clear.
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let url = self.kimi.get_url().await;
            let (turns, _) = self
                .kimi
                .eval_js("document.querySelectorAll('.chat-turn-container').length")
                .await;
            if url.contains("/prompts/new_chat") && turns.trim().parse::<usize>().unwrap_or(1) == 0
            {
                // Re-wait for textarea hydration after the reset.
                self.ensure_tab().await?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// Get the current prompt title (H1).
    pub async fn current_title(&self) -> String {
        let (text, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const h1 = document.querySelector('h1');
                return h1 ? h1.textContent.trim() : '';
            })()"#,
            )
            .await;
        text
    }

    /// Click the "Get code" button and extract the code snippet.
    pub async fn get_code(&self) -> Result<String> {
        let (result, _) = self
            .kimi
            .eval_js(
                r#"(() => {
                const btn = Array.from(document.querySelectorAll('button'))
                    .find(b => b.textContent.includes('Get code'));
                if (!btn) return 'no_get_code_btn';
                btn.click();
                return 'clicked';
            })()"#,
            )
            .await;
        if result.contains("no_get_code_btn") {
            return Err(AdapterError::ElementNotFound {
                selector: "Get code button".into(),
            });
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

        let items: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
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
            return Err(AdapterError::SendFailed {
                reason: format!("prompt '{}' not found in history", name),
            });
        }
        tokio::time::sleep(Duration::from_millis(2000)).await;
        Ok(())
    }

    /// Check if the page has an active response (model turn present).
    pub async fn has_response(&self) -> bool {
        let (text, _) = self
            .kimi
            .eval_js("document.querySelectorAll('.chat-turn-container.model').length")
            .await;
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
