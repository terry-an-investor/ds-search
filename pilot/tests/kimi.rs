//! Integration tests for the pilot crate (Kimi WebBridge HTTP client).
//!
//! Uses a mock HTTP server to exercise KimiPrimitives without a live browser.
//! Focus: eval_js/eval_json edge cases, error propagation, tab helpers.

use pilot::KimiPrimitives;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ── Mock harness ──

/// Responder closure type: maps (action, args) → response data value.
/// Factored into a type alias because the boxed trait-object form is
/// awkward to repeat and trips clippy's `type_complexity` lint.
type Responder = Arc<Mutex<Box<dyn Fn(&str, &Value) -> Value + Send + Sync>>>;

/// A mock Kimi WebBridge that returns canned responses keyed by a closure.
struct MockBridge {
    server: MockServer,
    /// When set, overrides the entire envelope (used to inject `ok:false`
    /// or top-level `error` fields that `_kimi` reads).
    envelope_override: Arc<Mutex<Option<Value>>>,
    /// HTTP status to return.
    status_override: Arc<Mutex<Option<u16>>>,
    /// When set, return 502 for the first N requests, then fall back to
    /// status_override / normal 200. Used to test read-only retry recovery.
    transient_fail_count: Arc<Mutex<u32>>,
    /// Closure that maps (action, args) → response `data` value.
    responder: Responder,
}

impl MockBridge {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let envelope_override = Arc::new(Mutex::new(None));
        let status_override = Arc::new(Mutex::new(None));
        let transient_fail_count = Arc::new(Mutex::new(0u32));
        // Default responder: echo back an empty object.
        let responder: Responder = Arc::new(Mutex::new(Box::new(|_action, _args| json!({}))));

        let env_clone = envelope_override.clone();
        let status_clone = status_override.clone();
        let fail_clone = transient_fail_count.clone();
        let resp_clone = responder.clone();

        Mock::given(method("POST"))
            .and(path("/command"))
            .respond_with(move |req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or_default();
                let action = body["action"].as_str().unwrap_or("");
                let args = &body["args"];

                // Transient failure injection: burn through the counter first.
                {
                    let mut fc = fail_clone.lock().unwrap();
                    if *fc > 0 {
                        *fc -= 1;
                        return ResponseTemplate::new(502)
                            .set_body_json(json!({"ok": false, "error": "HTTP error"}));
                    }
                }

                let status_code = status_clone.lock().unwrap().unwrap_or(200);
                // If an envelope override is set (e.g. for error injection), use it directly.
                if let Some(env) = env_clone.lock().unwrap().clone() {
                    return ResponseTemplate::new(status_code).set_body_json(env);
                }
                let data = {
                    let r = resp_clone.lock().unwrap();
                    r(action, args)
                };
                ResponseTemplate::new(status_code).set_body_json(json!({
                    "ok": true,
                    "data": data,
                }))
            })
            .mount(&server)
            .await;

        Self {
            server,
            envelope_override,
            status_override,
            transient_fail_count,
            responder,
        }
    }

    /// Set the closure that maps (action, args) → response data.
    fn respond_with<F>(&self, f: F)
    where
        F: Fn(&str, &Value) -> Value + Send + Sync + 'static,
    {
        *self.responder.lock().unwrap() = Box::new(f);
    }

    /// Inject a full envelope, e.g. `{"ok": false, "error": "boom"}`.
    /// `_kimi` reads the top-level `error` field on the `ok:false` path.
    fn force_envelope(&self, envelope: Value) {
        *self.envelope_override.lock().unwrap() = Some(envelope);
    }

    /// Force an HTTP error status.
    fn force_http_status(&self, code: u16) {
        *self.status_override.lock().unwrap() = Some(code);
    }

    /// Make the next `n` requests return 502, then recover. Used to test
    /// the read-only retry path.
    fn fail_n_then_recover(&self, n: u32) {
        *self.transient_fail_count.lock().unwrap() = n;
    }

    fn client(&self) -> KimiPrimitives {
        KimiPrimitives::new(self.server.uri(), "test-session")
    }
}

// ── eval_js: value type coverage ──

#[tokio::test]
async fn eval_js_string_value() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "hello world"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let (val, code) = k.eval_js("'hello'").await;
    assert_eq!(code, 0);
    assert_eq!(val, "hello world");
}

#[tokio::test]
async fn eval_js_null_value_returns_empty() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": null})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let (val, code) = k.eval_js("null").await;
    assert_eq!(code, 0);
    assert_eq!(val, "");
}

#[tokio::test]
async fn eval_js_bool_true() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": true})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let (val, code) = k.eval_js("true").await;
    assert_eq!(code, 0);
    assert_eq!(val, "true");
}

#[tokio::test]
async fn eval_js_bool_false() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": false})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let (val, code) = k.eval_js("false").await;
    assert_eq!(code, 0);
    assert_eq!(val, "false");
}

#[tokio::test]
async fn eval_js_number_value() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": 42})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let (val, code) = k.eval_js("42").await;
    assert_eq!(code, 0);
    assert_eq!(val, "42");
}

#[tokio::test]
async fn eval_js_missing_value_field() {
    let m = MockBridge::new().await;
    m.respond_with(|_action, _args| json!({})); // no "value" key
    let k = m.client();
    let (val, code) = k.eval_js("undefined").await;
    assert_eq!(code, 0);
    assert_eq!(val, "");
}

#[tokio::test]
async fn eval_js_on_kimi_error_returns_exit_1() {
    let m = MockBridge::new().await;
    // `_kimi` reads the top-level `error` field on the ok:false path.
    m.force_envelope(json!({"ok": false, "error": "boom"}));
    let k = m.client();
    let (val, code) = k.eval_js("broken").await;
    assert_eq!(code, 1);
    assert!(val.contains("boom"));
}

#[tokio::test]
async fn eval_js_on_http_error_returns_exit_1() {
    let m = MockBridge::new().await;
    m.respond_with(|_action, _args| json!({}));
    m.force_http_status(502);
    let k = m.client();
    let (val, code) = k.eval_js("502").await;
    assert_eq!(code, 1);
    assert!(val.contains("502"));
}

// ── eval_json: parsing paths ──

#[tokio::test]
async fn eval_json_object_direct() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "{\"a\":1,\"b\":2}"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let v = k.eval_json("JSON.stringify({a:1})").await.unwrap();
    assert_eq!(v["a"], json!(1));
    assert_eq!(v["b"], json!(2));
}

#[tokio::test]
async fn eval_json_array_direct() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "[1,2,3]"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let v = k.eval_json("JSON.stringify([1,2,3])").await.unwrap();
    assert_eq!(v, json!([1, 2, 3]));
}

/// The double-wrapped case: WebBridge returns a JSON-stringified object
/// as a quoted string, i.e. `"{\"key\":\"val\"}"`.
#[tokio::test]
async fn eval_json_double_wrapped_string() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            // Outer quotes + escaped inner JSON
            json!({"value": "{\"key\":\"val\"}"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let v = k.eval_json("JSON.stringify({key:'val'})").await.unwrap();
    assert_eq!(v["key"], json!("val"));
}

#[tokio::test]
async fn eval_json_empty_returns_none() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": ""})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(k.eval_json("''").await.is_none());
}

#[tokio::test]
async fn eval_json_invalid_json_returns_none() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "not json at all"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(k.eval_json("garbage").await.is_none());
}

/// A bare string scalar (not object/array) is not a valid JSON payload
/// for our purposes — eval_json should return None.
#[tokio::test]
async fn eval_json_bare_scalar_returns_none() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "hello"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(k.eval_json("'hello'").await.is_none());
}

#[tokio::test]
async fn eval_json_on_error_returns_none() {
    let m = MockBridge::new().await;
    m.force_envelope(json!({"ok": false, "error": "fail"}));
    let k = m.client();
    assert!(k.eval_json("broken").await.is_none());
}

// ── navigate / get_url ──

#[tokio::test]
async fn navigate_returns_data_on_success() {
    let m = MockBridge::new().await;
    m.respond_with(|action, args| {
        if action == "navigate" {
            json!({"url": args["url"], "success": true})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let data = k.navigate("https://example.com", true).await.unwrap();
    assert_eq!(data["success"], json!(true));
}

#[tokio::test]
async fn navigate_propagates_kimi_error() {
    let m = MockBridge::new().await;
    m.force_envelope(json!({"ok": false, "error": "nav failed"}));
    let k = m.client();
    let err = k.navigate("https://example.com", false).await.unwrap_err();
    assert!(err.to_string().contains("nav failed"));
}

#[tokio::test]
async fn get_url_reads_location_href() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _args| {
        if action == "evaluate" {
            json!({"value": "https://current.page/foo"})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert_eq!(k.get_url().await, "https://current.page/foo");
}

// ── find_tab / list_tabs ──

#[tokio::test]
async fn find_tab_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _args| {
        if action == "find_tab" {
            json!({"success": true})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(k.find_tab("deepseek").await);
}

#[tokio::test]
async fn find_tab_not_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _args| {
        if action == "find_tab" {
            json!({"success": false})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(!k.find_tab("nope").await);
}

#[tokio::test]
async fn list_tabs_parses_tab_array() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _args| {
        if action == "list_tabs" {
            json!({"tabs": [
                {"tabId": 1, "url": "https://a.com", "title": "A", "active": true},
                {"tabId": 2, "url": "https://b.com", "title": "B", "active": false},
            ]})
        } else {
            json!({})
        }
    });
    let k = m.client();
    let tabs = k.list_tabs().await;
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].tab_id, 1);
    assert_eq!(tabs[0].url, "https://a.com");
    assert!(tabs[0].active);
    assert_eq!(tabs[1].title, "B");
    assert!(!tabs[1].active);
}

#[tokio::test]
async fn list_tabs_empty() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _args| {
        if action == "list_tabs" {
            json!({"tabs": []})
        } else {
            json!({})
        }
    });
    let k = m.client();
    assert!(k.list_tabs().await.is_empty());
}

#[tokio::test]
async fn list_tabs_missing_tabs_field() {
    let m = MockBridge::new().await;
    m.respond_with(|_action, _args| json!({}));
    let k = m.client();
    assert!(k.list_tabs().await.is_empty());
}

// ── session accessor ──

#[tokio::test]
async fn session_name_preserved() {
    let m = MockBridge::new().await;
    let k = KimiPrimitives::new(m.server.uri(), "my-session");
    assert_eq!(k.session(), "my-session");
}

// ── read-only retry on transient 502 ──

/// eval_js (read-only) should retry past a transient 502 and succeed once
/// the mock recovers.
#[tokio::test]
async fn eval_js_retries_past_transient_502() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "recovered"})
        } else {
            json!({})
        }
    });
    // Fail the first 2 attempts, then the 3rd succeeds (within READONLY_RETRY_MAX=3).
    m.fail_n_then_recover(2);
    let (val, code) = m.client().eval_js("'x'").await;
    assert_eq!(code, 0);
    assert_eq!(val, "recovered");
}

/// If the transient failure outlasts the retry budget, eval_js reports the
/// error rather than looping forever.
#[tokio::test]
async fn eval_js_gives_up_after_retry_budget() {
    let m = MockBridge::new().await;
    // More failures than READONLY_RETRY_MAX → exhausted.
    m.fail_n_then_recover(10);
    let (val, code) = m.client().eval_js("'x'").await;
    assert_eq!(code, 1);
    assert!(val.contains("502"));
}

/// Business errors (ok:false) are NOT transient and must not be retried —
/// retrying a business failure would waste calls.
#[tokio::test]
async fn eval_js_does_not_retry_business_error() {
    let m = MockBridge::new().await;
    m.force_envelope(json!({"ok": false, "error": "boom"}));
    let (val, code) = m.client().eval_js("broken").await;
    assert_eq!(code, 1);
    assert!(val.contains("boom"));
    // Only one request should have been served (no retries).
    assert_eq!(m.server.received_requests().await.unwrap().len(), 1);
}

/// list_tabs (read-only) should also recover from transient 502.
#[tokio::test]
async fn list_tabs_retries_past_transient_502() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "list_tabs" {
            json!({"tabs": [{"tabId": 1, "url": "https://x.com", "title": "X", "active": true}]})
        } else {
            json!({})
        }
    });
    m.fail_n_then_recover(1);
    let tabs = m.client().list_tabs().await;
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].url, "https://x.com");
}
