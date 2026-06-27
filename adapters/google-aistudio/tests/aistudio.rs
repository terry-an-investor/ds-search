//! Integration tests for the google-aistudio adapter via a mock Kimi WebBridge.
//!
//! Mirrors the MockBridge harness used by adapters/gemini and adapters/grok.
//! All adapter methods talk to the browser through POST /command with
//! {"action","args","session"}; we stub that endpoint and return canned JS-eval
//! payloads so the adapter's parsing and control-flow logic is exercised.

use google_aistudio::{AistudioModel, AistudioSemantics, ThinkingLevel, Tool, TurnRole};
use pilot::KimiPrimitives;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ── Mock harness (mirrors gemini/grok tests) ──

/// Responder closure type (aliased to satisfy clippy's type_complexity lint).
type Responder = Arc<Mutex<Box<dyn Fn(&str, &Value) -> Value + Send + Sync>>>;

struct MockBridge {
    server: MockServer,
    responder: Responder,
}

impl MockBridge {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let responder: Responder = Arc::new(Mutex::new(Box::new(|_, _| json!({}))));

        let resp_clone = responder.clone();
        Mock::given(method("POST"))
            .and(path("/command"))
            .respond_with(move |req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or_default();
                let action = body["action"].as_str().unwrap_or("");
                let args = &body["args"];
                let data = {
                    let r = resp_clone.lock().unwrap();
                    r(action, args)
                };
                ResponseTemplate::new(200).set_body_json(json!({"ok": true, "data": data}))
            })
            .mount(&server)
            .await;

        Self { server, responder }
    }

    fn respond_with<F>(&self, f: F)
    where
        F: Fn(&str, &Value) -> Value + Send + Sync + 'static,
    {
        *self.responder.lock().unwrap() = Box::new(f);
    }

    fn client(&self) -> AistudioSemantics {
        AistudioSemantics::new(KimiPrimitives::new(self.server.uri(), "aistudio-test"))
    }
}

// Helper: a JS-eval responder that returns the given string value.
fn eval_value(s: &str) -> Value {
    json!({"value": s})
}

// ── Model enum ──

#[test]
fn model_labels_roundtrip() {
    let cases = [
        (AistudioModel::FlashLite, "flash-lite"),
        (AistudioModel::Flash, "flash"),
        (AistudioModel::Pro, "pro"),
        (AistudioModel::ProLatest, "pro-latest"),
        (AistudioModel::FlashLatest, "flash-latest"),
        (AistudioModel::FlashLiteLatest, "flash-lite-latest"),
    ];
    for (m, label) in cases {
        assert_eq!(
            AistudioModel::from_label(label),
            Some(m),
            "label {label} should map back to the model"
        );
    }
    assert_eq!(AistudioModel::from_label("nonsense"), None);
}

// ── Tool enum ──

#[test]
fn tool_labels_roundtrip() {
    assert_eq!(
        Tool::from_label("search"),
        Some(Tool::GoogleSearchGrounding)
    );
    assert_eq!(Tool::from_label("code"), Some(Tool::CodeExecution));
    assert_eq!(Tool::from_label("function"), Some(Tool::FunctionCalling));
    assert_eq!(Tool::from_label("maps"), Some(Tool::MapsGrounding));
    assert_eq!(Tool::from_label("url"), Some(Tool::UrlContext));
    assert_eq!(
        Tool::from_label("structured"),
        Some(Tool::StructuredOutputs)
    );
    assert_eq!(Tool::from_label("nope"), None);
    assert!(
        Tool::GoogleSearchGrounding
            .as_label()
            .contains("Grounding with Google Search")
    );
}

// ── ThinkingLevel enum ──

#[test]
fn thinking_level_roundtrip() {
    assert_eq!(ThinkingLevel::from_label("low"), Some(ThinkingLevel::Low));
    assert_eq!(ThinkingLevel::from_label("HIGH"), Some(ThinkingLevel::High));
    assert_eq!(ThinkingLevel::Medium.as_label(), "Medium");
    assert_eq!(ThinkingLevel::from_label("x"), None);
}

// ── get_state ──

#[tokio::test]
async fn get_state_parses_all_fields() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value(
                &json!({
                    "url": "https://aistudio.google.com/prompts/abc",
                    "is_on_playground": true,
                    "has_input": true,
                    "is_streaming": false,
                    "user_turn_count": 2,
                    "model_turn_count": 2,
                    "current_model": "Gemini 3 Pro"
                })
                .to_string(),
            )
        } else {
            json!({})
        }
    });
    let s = m.client().get_state().await;
    assert!(s.is_on_playground);
    assert!(s.has_input);
    assert!(!s.is_streaming);
    assert_eq!(s.user_turn_count, 2);
    assert_eq!(s.model_turn_count, 2);
    assert_eq!(s.current_model, "Gemini 3 Pro");
}

#[tokio::test]
async fn get_state_defaults_on_empty_response() {
    let m = MockBridge::new().await;
    let s = m.client().get_state().await;
    assert!(!s.is_on_playground);
    assert!(!s.has_input);
    assert_eq!(s.user_turn_count, 0);
    assert!(s.url.is_empty());
}

// ── extract_turns ──

#[tokio::test]
async fn extract_turns_parses_user_and_model() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            // extract_turns fires two evals: scroll_chat (no value) then the JSON.
            // The harness returns the same payload for both; the JSON parse only
            // succeeds on the array, so the scroll call is harmlessly ignored.
            eval_value(
                &json!([
                    {"role": "user", "content": "Hello"},
                    {"role": "model", "content": "Hi there!"}
                ])
                .to_string(),
            )
        } else {
            json!({})
        }
    });
    let turns = m.client().extract_turns().await;
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].role, TurnRole::User);
    assert_eq!(turns[0].content, "Hello");
    assert_eq!(turns[1].role, TurnRole::Model);
    assert_eq!(turns[1].content, "Hi there!");
}

#[tokio::test]
async fn extract_turns_empty_when_no_json() {
    let m = MockBridge::new().await;
    // Default responder returns {} — not an array, so eval_json yields None.
    let turns = m.client().extract_turns().await;
    assert!(turns.is_empty());
}

// ── extract_thinking ──

#[tokio::test]
async fn extract_thinking_present() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("Let me reason step by step...")
        } else {
            json!({})
        }
    });
    let th = m.client().extract_thinking().await;
    assert!(th.is_some());
    assert!(th.unwrap().contains("reason step by step"));
}

#[tokio::test]
async fn extract_thinking_none_when_empty() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("")
        } else {
            json!({})
        }
    });
    assert!(m.client().extract_thinking().await.is_none());
}

// ── last_response_runtime ──

#[tokio::test]
async fn runtime_parses_pill() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("3.9s")
        } else {
            json!({})
        }
    });
    assert_eq!(
        m.client().last_response_runtime().await.as_deref(),
        Some("3.9s")
    );
}

#[tokio::test]
async fn runtime_none_when_invalid() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("not a pill")
        } else {
            json!({})
        }
    });
    assert!(m.client().last_response_runtime().await.is_none());
}

// ── temperature ──

#[tokio::test]
async fn get_temperature_parses_number() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("0.7")
        } else {
            json!({})
        }
    });
    assert_eq!(m.client().get_temperature().await, Some(0.7));
}

#[tokio::test]
async fn get_temperature_none_when_garbage() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("warm")
        } else {
            json!({})
        }
    });
    assert!(m.client().get_temperature().await.is_none());
}

// ── set_system_instructions ──

#[tokio::test]
async fn set_system_instructions_ok_when_target_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("ok")
        } else {
            json!({})
        }
    });
    assert!(
        m.client()
            .set_system_instructions("be concise")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn set_system_instructions_errors_when_no_target() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("no_target")
        } else {
            json!({})
        }
    });
    let err = m.client().set_system_instructions("x").await.unwrap_err();
    assert!(err.to_string().contains("system instructions"));
}

// ── toggle_tool ──

#[tokio::test]
async fn toggle_tool_ok_when_clicked() {
    let m = MockBridge::new().await;
    // toggle_tool issues two evals: click ("clicked") then a state read.
    // The state read (is_tool_enabled) must return "true" for the post-toggle
    // state. The expand_tools_section pre-check also evals; return true for it.
    m.respond_with(|action, args| {
        if action != "evaluate" {
            return json!({});
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if code.contains("tools-dialog") {
            return eval_value("true"); // already expanded
        }
        if code.contains("tool-item") && code.contains("click") {
            return eval_value("clicked");
        }
        if code.contains("mdc-switch--checked") || code.contains("aria-checked") {
            return eval_value("true"); // post-toggle enabled
        }
        eval_value("clicked")
    });
    assert!(
        m.client()
            .toggle_tool(Tool::GoogleSearchGrounding)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn toggle_tool_errors_when_not_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("not_found")
        } else {
            json!({})
        }
    });
    let err = m
        .client()
        .toggle_tool(Tool::CodeExecution)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("tool item"));
}

// ── send_prompt (hardened) ──

/// Simulates a successful send through the full pipeline:
///   ensure_tab → dismiss_dialogs → fill_and_verify (DomState JSON) → click_run → VERIFY turn rise.
/// We distinguish evals by their `code` payload (args.code).
#[tokio::test]
async fn send_prompt_succeeds_when_user_turn_appears() {
    let m = MockBridge::new().await;
    let user_turn_probes = Arc::new(Mutex::new(0u8)); // counts user_turn_count probes
    let probes_clone = user_turn_probes.clone();
    m.respond_with(move |action, args| {
        if action != "evaluate" {
            return json!({}); // navigate / key_type → no value needed
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        // ensure_tab / get_url: on-domain URL.
        if code.contains("window.location.href") && !code.contains("JSON.stringify") {
            return eval_value("https://aistudio.google.com/prompts/new_chat");
        }
        // ensure_tab textarea-presence probe.
        if code.contains("!!document.querySelector('textarea')") {
            return eval_value("true");
        }
        // user_turn_count() direct eval (send VERIFY): count rises 1→2 after click.
        if code.contains("chat-turn-container.user") && !code.contains("JSON.stringify") {
            let mut p = probes_clone.lock().unwrap();
            *p += 1;
            let n = if *p <= 2 { 1 } else { 2 };
            return json!({"value": n.to_string()});
        }
        // DomState::read JSON snapshot — must carry the filled text so the
        // fill_and_verify predicate (extra == "hello") passes.  The extra field
        // is the prompt-box value (read via extra_js), because the generic
        // textarea_value only reads the first textarea (system instructions).
        if code.contains("JSON.stringify") {
            return eval_value(
                &json!({
                    "textarea_value": "",       // first textarea (system inst.)
                    "user_turn_count": 1,
                    "model_turn_count": 0,
                    "url": "https://aistudio.google.com/prompts/new_chat",
                    "title": "",
                    "is_streaming": false,
                    "extra": "hello"            // the PROMPT box value
                })
                .to_string(),
            );
        }
        // click_run / nativeSetter fill / Enter → any non-error string.
        eval_value("ok")
    });
    let res = m.client().send_prompt("hello").await;
    assert!(res.is_ok(), "expected Ok, got: {:?}", res);
    assert!(*user_turn_probes.lock().unwrap() >= 2);
}

/// Simulates a failed send: the fill VERIFY fails because the textarea never
/// receives the text (eval_json returns non-JSON → default DomState → empty
/// textarea → predicate false). The pipeline surfaces VerifyFailed.
#[tokio::test]
async fn send_prompt_fails_when_fill_not_verified() {
    let m = MockBridge::new().await;
    // Every evaluate returns a bare string — eval_json yields None → DomState
    // default → textarea_value "" != "hello" → fill_and_verify VerifyFailed.
    m.respond_with(|_action, _args| json!({"value": "5"}));
    let err = m.client().send_prompt("hello").await.unwrap_err();
    assert!(
        err.to_string().contains("VERIFY failed") || err.to_string().contains("fill"),
        "expected VerifyFailed from fill, got: {err}"
    );
}

// ── wait_for_response ──

/// A length sequence that stabilizes → returns true.
#[tokio::test]
async fn wait_for_response_detects_stability() {
    let m = MockBridge::new().await;
    let lengths = Arc::new(Mutex::new(
        vec![0u64, 100, 200, 300, 300, 300, 300].into_iter(),
    ));
    let lengths_clone = lengths.clone();
    m.respond_with(move |_action, _args| {
        let len = lengths_clone.lock().unwrap().next().unwrap_or(300);
        json!({"value": len.to_string()})
    });
    let ready = m.client().wait_for_response(5).await;
    assert!(ready.is_ok_and(|ok| ok));
}

#[tokio::test]
async fn wait_for_response_times_out_when_never_stable() {
    let m = MockBridge::new().await;
    // Always-zero length: never stabilizes at a positive value, and the
    // timeout path returns Ok(len > 0) = Ok(false).
    m.respond_with(|_action, _args| json!({"value": "0"}));
    let ready = m.client().wait_for_response(1).await;
    assert!(ready.is_ok_and(|ok| !ok));
}

// ── check_generation_error ──

#[tokio::test]
async fn check_error_detects_explicit_error_text() {
    let m = MockBridge::new().await;
    m.respond_with(|action, args| {
        if action != "evaluate" {
            return json!({});
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if code.contains("try again") {
            return eval_value("try again");
        }
        json!({"value": "0"})
    });
    // The error-pattern probe returns "try again" → Some(error).
    let err = m.client().check_generation_error().await;
    assert!(err.is_some());
    assert!(err.unwrap().contains("try again"));
}

#[tokio::test]
async fn check_error_detects_missing_pill() {
    let m = MockBridge::new().await;
    m.respond_with(|action, args| {
        if action != "evaluate" {
            return json!({});
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        // error-pattern probe returns '' (no error text)
        if code.contains("try again") {
            return eval_value("");
        }
        // model turn count probe → 1 (a turn exists)
        if code.contains("chat-turn-container.model") {
            return json!({"value": "1"});
        }
        // pill probe → '' (no pill → failure)
        if code.contains("model-run-time-pill") {
            return eval_value("");
        }
        json!({"value": "0"})
    });
    let err = m.client().check_generation_error().await;
    assert!(err.is_some(), "missing pill should be detected as failure");
    assert!(err.unwrap().contains("no run-time pill"));
}

#[tokio::test]
async fn check_error_none_when_pill_present() {
    let m = MockBridge::new().await;
    m.respond_with(|action, args| {
        if action != "evaluate" {
            return json!({});
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if code.contains("try again") {
            return eval_value("");
        }
        if code.contains("chat-turn-container.model") {
            return json!({"value": "1"});
        }
        if code.contains("model-run-time-pill") {
            return eval_value("3.7s");
        }
        json!({"value": "0"})
    });
    assert!(m.client().check_generation_error().await.is_none());
}

// ── rerun_last_turn ──

#[tokio::test]
async fn rerun_clicks_button_when_present() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("rerun")
        } else {
            json!({})
        }
    });
    assert!(m.client().rerun_last_turn().await.is_ok());
}

#[tokio::test]
async fn rerun_errors_when_no_model_turn() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("no_model_turn")
        } else {
            json!({})
        }
    });
    let err = m.client().rerun_last_turn().await.unwrap_err();
    assert!(err.to_string().contains("model turn"));
}

#[tokio::test]
async fn rerun_errors_when_no_button() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("no_rerun_button")
        } else {
            json!({})
        }
    });
    let err = m.client().rerun_last_turn().await.unwrap_err();
    assert!(err.to_string().contains("Rerun"));
}

// ── rate_response ──

#[tokio::test]
async fn rate_up_clicks_good_response() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("clicked")
        } else {
            json!({})
        }
    });
    assert!(m.client().rate_response(true).await.unwrap());
}

#[tokio::test]
async fn rate_errors_when_no_model_turn() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("no_model_turn")
        } else {
            json!({})
        }
    });
    let err = m.client().rate_response(false).await.unwrap_err();
    assert!(err.to_string().contains("model turn"));
}

// ── share ──

#[tokio::test]
async fn share_clicks_when_present() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("clicked")
        } else {
            json!({})
        }
    });
    assert!(m.client().share().await.unwrap());
}

#[tokio::test]
async fn share_errors_when_absent() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("not_found")
        } else {
            json!({})
        }
    });
    let err = m.client().share().await.unwrap_err();
    assert!(err.to_string().contains("Share prompt"));
}

#[tokio::test]
async fn get_share_link_parses_value() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            eval_value("https://aistudio.google.com/some-shared-id")
        } else {
            json!({})
        }
    });
    assert_eq!(
        m.client().get_share_link().await.as_deref(),
        Some("https://aistudio.google.com/some-shared-id")
    );
}

// ── Bug #3 regression: last-iteration recovery not discarded ──
// wait_for_response_or_rerun must return extracted content even when the
// generation-error check misfires on the final iteration. Previously it
// returned NoResponse as soon as check_generation_error() was Some on the
// last attempt, throwing away a recovery that actually produced text.
#[tokio::test]
async fn rerun_loop_returns_content_even_when_error_check_misfires() {
    let m = MockBridge::new().await;
    let extracts = Arc::new(Mutex::new(0u32));
    let extracts_clone = extracts.clone();
    m.respond_with(move |action, args| {
        if action != "evaluate" {
            return json!({});
        }
        let code = args.get("code").and_then(|c| c.as_str()).unwrap_or("");
        // IMPORTANT: check the most-specific patterns first. extract_response's
        // JS contains BOTH '.chat-turn-container.model' AND '.text-chunk', so the
        // .text-chunk branch must win or the extract reads the model-turn count.
        // extract_response reads `.text-chunk`. Empty on iteration 0 (so it
        // reruns), content on iteration 1+ (the fix returns it instead of
        // NoResponse). This is the exact Bug #3 scenario.
        if code.contains(".text-chunk") {
            let mut n = extracts_clone.lock().unwrap();
            *n += 1;
            return eval_value(if *n >= 2 { "recovered reply" } else { "" });
        }
        // wait_for_response length probe: stable non-zero → settles immediately.
        if code.contains("last-of-type") {
            return json!({"value": "42"});
        }
        // check_generation_error error-pattern probe → "try again" (mimics a
        // lingering transient error). Makes the error branch reachable on every
        // iteration including the last.
        if code.contains("try again") {
            return eval_value("try again");
        }
        // rerun_last_turn: JS contains 'chat-turn-container.model' AND
        // 'Rerun this turn' — check the rerun-specific marker FIRST so it wins
        // over the generic model-count probe.
        if code.contains("Rerun this turn") || code.contains("rerun-button") {
            return eval_value("rerun");
        }
        // model turn count probe → 1.
        if code.contains("chat-turn-container.model") {
            return json!({"value": "1"});
        }
        // pill probe → "" (no pill → error confirmed by check_generation_error).
        if code.contains("model-run-time-pill") {
            return eval_value("");
        }
        json!({"value": "0"})
    });
    // max_retries=1 → iterations 0 and 1. Iter 0: error detected, empty extract
    // → rerun. Iter 1 (last): error STILL misfires, BUT extract returns content.
    let res = m.client().wait_for_response_or_rerun(1).await;
    assert!(res.is_ok(), "expected recovered reply, got: {:?}", res);
    assert_eq!(res.unwrap(), "recovered reply");
}
