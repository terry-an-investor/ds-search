//! Verification primitives — encode the AGENTS.md "observe → act → verify"
//! discipline as code so it can't be skipped by accident.
//!
//! ## Why this exists
//!
//! Every layer-2 adapter kept repeating the same class of bug: an ACT method
//! (fill, click, send) returned `Ok(())`, the caller trusted the return value,
//! and moved on — without ever reading the DOM back. The return value only
//! proves the *HTTP* call succeeded, not that the *page* changed. Real bugs
//! this caused:
//!
//! - `dismiss_dialogs` returned ok but had dispatched an unconditional `Escape`
//!   that silently wiped the textarea → every subsequent send "not accepted".
//! - `set_prompt_text` returned ok but the Angular form never received the
//!   value → clicking Run was a no-op.
//! - `toggle_tool` returned `clicked` but the selector pointed at nothing → the
//!   switch never moved.
//!
//! None of these are detectable from a return value. They are only detectable
//! by re-reading the DOM after the action. This module turns that re-read into
//! a method you call on the same line as the act, so skipping it is a compile
//! or lint smell, not a discipline lapse.
//!
//! ## How to use
//!
//! ```ignore
//! use pilot::verify::VerifyDriven;
//!
//! // fill, then prove the text landed before doing anything else
//! verifier.fill_and_verify(&kimi, selector, text, |after| after.value_len() > 0).await?;
//!
//! // click, then prove the page reacted
//! kimi.click_and_verify(selector, |after| after.url != before_url).await?;
//! ```
//!
//! The closure receives a freshly-read [`DomState`] snapshot. If it returns
//! false, you get a [`VerifyFailed`](crate::error::AdapterError::VerifyFailed)
//! with the before/after diff — never a silent `Ok(())`.

// async-in-trait here is intentional and fine under the MSRV; silence the lint
// rather than pull in the async-trait dependency.
#![allow(async_fn_in_trait)]

use crate::error::{AdapterError, Result};
use crate::kimi::KimiPrimitives;
use std::time::Duration;
use tracing::{debug, warn};

/// A freshly-read DOM snapshot used as the VERIFY input.
///
/// Built by a single `eval_json` call so verification costs one round-trip,
/// not several. Fields are `Option`/`String` because every site differs; the
/// closure decides what to assert.
#[derive(Debug, Clone, Default)]
pub struct DomState {
    /// `document.querySelector('textarea')?.value || ''`
    pub textarea_value: String,
    /// `.chat-turn-container.user` count (or any `.user-turn`-style marker).
    pub user_turn_count: usize,
    /// `.chat-turn-container.model` count.
    pub model_turn_count: usize,
    /// `window.location.href`.
    pub url: String,
    /// `document.title`.
    pub title: String,
    /// True if a spinner / `[aria-busy=true]` / `[role=progressbar]` is present.
    pub is_streaming: bool,
    /// Free-form bag for site-specific fields (e.g. an aria-pressed state).
    pub extra: serde_json::Value,
}

impl DomState {
    /// Convenience: did the textarea actually get the text?
    pub fn value_is(&self, expected: &str) -> bool {
        self.textarea_value == expected
    }
    /// Convenience: textarea non-empty after a fill.
    pub fn value_present(&self) -> bool {
        !self.textarea_value.is_empty()
    }
    /// Convenience: textarea length (for `> 0` style predicates).
    pub fn value_len(&self) -> usize {
        self.textarea_value.len()
    }
}

impl DomState {
    /// Read a full snapshot in one `eval_json` round-trip.
    ///
    /// The selectors are the common-denominator ones shared across the AI-chat
    /// sites in this repo (deepseek/grok/gemini/aistudio all use
    /// `.chat-turn-container{.user,.model}`). For sites that differ, pass an
    /// `extra_js` snippet whose result lands in [`DomState::extra`].
    pub async fn read(kimi: &KimiPrimitives, extra_js: Option<&str>) -> Self {
        let extra = extra_js.unwrap_or("null");
        let v = kimi
            .eval_json(&format!(
                r#"JSON.stringify((() => {{
                const ta = document.querySelector('textarea');
                const spin = document.querySelector(
                    '.mat-progress-spinner, [role="progressbar"], [aria-busy="true"], .loading, .spinner'
                );
                return {{
                    textarea_value: ta ? (ta.value || '') : '',
                    user_turn_count: document.querySelectorAll('.chat-turn-container.user, .user-turn').length,
                    model_turn_count: document.querySelectorAll('.chat-turn-container.model, .model-turn').length,
                    url: window.location.href,
                    title: document.title || '',
                    is_streaming: !!spin,
                    extra: {extra}
                }};
            }})())"#
            ))
            .await;
        match v {
            Some(v) => Self {
                textarea_value: v
                    .get("textarea_value")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
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
                url: v
                    .get("url")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: v
                    .get("title")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                is_streaming: v
                    .get("is_streaming")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                extra: v.get("extra").cloned().unwrap_or(serde_json::Value::Null),
            },
            None => Self::default(),
        }
    }
}

/// Extension trait: VERIFY-driven variants of the common ACTs.
///
/// Each method performs the act, then reads a [`DomState`] and runs the
/// supplied predicate. A failing predicate returns
/// [`AdapterError::VerifyFailed`] with a human-readable diff — it never
/// silently returns `Ok(())`. This is the codified AGENTS.md rule
/// "禁止不验证就进入下一步".
pub trait VerifyDriven {
    /// The primitives handle this adapter talks to.
    fn kimi(&self) -> &KimiPrimitives;

    /// ACT: run `js` (a mutating script). VERIFY: read state, assert `check`.
    ///
    /// Use this when the ACT is a custom `eval_js` (e.g. nativeSetter fill,
    /// click-via-querySelector). The `label` shows up in error messages so the
    /// diff is actionable.
    async fn act_and_verify<F>(
        &self,
        label: &str,
        js: &str,
        extra_js: Option<&str>,
        check: F,
    ) -> Result<DomState>
    where
        F: Fn(&DomState) -> bool + Send,
    {
        let kimi = self.kimi();
        let before = DomState::read(kimi, extra_js).await;
        let _ = kimi.eval_js(js).await;
        // small settle for Angular/React to flush
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = DomState::read(kimi, extra_js).await;
        if check(&after) {
            debug!(label = label, "verify ok");
            Ok(after)
        } else {
            warn!(label = label, before = ?before, after = ?after, "VERIFY FAILED");
            Err(AdapterError::VerifyFailed {
                action: label.to_string(),
                reason: format!(
                    "before turns={}/{} url={} val_len={} → after turns={}/{} url={} val_len={}",
                    before.user_turn_count,
                    before.model_turn_count,
                    before.url,
                    before.textarea_value.len(),
                    after.user_turn_count,
                    after.model_turn_count,
                    after.url,
                    after.textarea_value.len(),
                ),
            })
        }
    }

    /// Convenience: fill a textarea via nativeSetter and VERIFY the value
    /// landed. Uses the repo-standard selector `'textarea'`; pass a different
    /// one for sites whose prompt input is a contenteditable or a class.
    async fn fill_and_verify<F>(
        &self,
        selector: &str,
        text: &str,
        extra_js: Option<&str>,
        check: F,
    ) -> Result<DomState>
    where
        F: Fn(&DomState) -> bool + Send,
    {
        // Use JSON string escaping — handles \n, \t, ", \u2028, etc. The old
        // `replace('\\',"\\\\").replace('\'',"\\'")` only handled \ and ', so a
        // multi-line prompt ("line1\nline2") produced a literal newline inside
        // a single-quoted JS string → SyntaxError → silent fill failure.
        // Verified bug: multiline sends returned VerifyFailed (val_len=0).
        let escaped = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
        let js = format!(
            r#"(() => {{
                const el = document.querySelector('{selector}');
                if (!el) return 'no_target';
                if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {{
                    const s = Object.getOwnPropertyDescriptor(
                        el.tagName === 'TEXTAREA'
                            ? HTMLTextAreaElement.prototype
                            : HTMLInputElement.prototype,
                        'value'
                    ).set;
                    s.call(el, {escaped});
                }} else {{
                    el.textContent = {escaped};
                }}
                el.dispatchEvent(new Event('input', {{bubbles: true}}));
                el.dispatchEvent(new Event('change', {{bubbles: true}}));
                return 'ok';
            }})()"#
        );
        self.act_and_verify(&format!("fill('{selector}')"), &js, extra_js, check)
            .await
    }
}

/// Blanket impl: anything that exposes a `KimiPrimitives` ref gets the trait.
/// Today only the layer-2 adapters implement `kimi()`; this blanket keeps it
/// open without forcing a shared base struct.
impl<T> VerifyDriven for T
where
    T: KimiRef,
{
    fn kimi(&self) -> &KimiPrimitives {
        T::kimi_ref(self)
    }
}

/// Sealed-ish helper: a type that can hand back its `KimiPrimitives`.
/// Adapters implement this (one line) and get all of [`VerifyDriven`] free.
pub trait KimiRef {
    fn kimi_ref(&self) -> &KimiPrimitives;
}
