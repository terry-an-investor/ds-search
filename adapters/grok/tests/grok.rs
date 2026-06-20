//! Integration tests for the grok adapter via a mock Kimi WebBridge.

use grok::GrokSemantics;
use pilot::KimiPrimitives;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ── Mock harness (mirrors the pilot test harness) ──

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

    fn client(&self) -> GrokSemantics {
        GrokSemantics::new(KimiPrimitives::new(self.server.uri(), "grok-test"))
    }
}

// ── get_state ──

#[tokio::test]
async fn get_state_parses_all_fields() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": json!({
                "has_input": true,
                "has_conversation": true,
                "url": "https://x.com/i/grok?conversation=123",
                "is_initialized": true,
            }).to_string()})
        } else {
            json!({})
        }
    });
    let s = m.client().get_state().await;
    assert!(s.has_input);
    assert!(s.has_conversation);
    assert!(s.is_initialized);
    assert_eq!(s.url, "https://x.com/i/grok?conversation=123");
}

#[tokio::test]
async fn get_state_defaults_on_empty_response() {
    let m = MockBridge::new().await;
    // No evaluate response configured → eval_js returns ("", 0) → eval_json None → defaults
    let s = m.client().get_state().await;
    assert!(!s.has_input);
    assert!(!s.has_conversation);
    assert_eq!(s.url, "");
}

// ── send_message ──

#[tokio::test]
async fn send_message_empty_rejects() {
    let m = MockBridge::new().await;
    let err = m.client().send_message("   ").await.unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[tokio::test]
async fn send_message_whitespace_only_rejects() {
    let m = MockBridge::new().await;
    let err = m.client().send_message("\n\t ").await.unwrap_err();
    assert!(err.to_string().contains("empty"));
}

// ── extract_last_response ──

#[tokio::test]
async fn extract_last_response_with_text() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": json!({
                "found": true,
                "text": "Hello from Grok!",
            }).to_string()})
        } else {
            json!({})
        }
    });
    let resp = m.client().extract_last_response().await;
    assert_eq!(resp, "Hello from Grok!");
}

#[tokio::test]
async fn extract_last_response_empty_when_not_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": json!({"found": false, "text": ""}).to_string()})
        } else {
            json!({})
        }
    });
    let resp = m.client().extract_last_response().await;
    assert_eq!(resp, "");
}

// ── wait_for_response ──

/// Simulates a body-length sequence that stabilizes after a few polls.
#[tokio::test]
async fn wait_for_response_detects_stability() {
    let m = MockBridge::new().await;
    let lengths = Arc::new(Mutex::new(vec![100, 200, 300, 300, 300, 300].into_iter()));
    let lengths_clone = lengths.clone();
    m.respond_with(move |_action, _args| {
        let len = lengths_clone.lock().unwrap().next().unwrap_or(300);
        // Body-length is read as a string by grok's wait_for_response.
        json!({"value": len.to_string()})
    });
    // 4 stable rounds required; we feed 3 stable 300s at the end → returns true.
    let ready = m.client().wait_for_response(5).await;
    assert!(ready);
}

#[tokio::test]
async fn wait_for_response_times_out_when_never_stable() {
    let m = MockBridge::new().await;
    // Always-changing length → never stabilizes.
    let counter = Arc::new(Mutex::new(0u32));
    let counter_clone = counter.clone();
    m.respond_with(move |_action, _args| {
        let mut c = counter_clone.lock().unwrap();
        *c += 1;
        json!({"value": c.to_string()})
    });
    let ready = m.client().wait_for_response(1).await;
    assert!(!ready);
}

// ── Model enum ──

#[tokio::test]
async fn model_label() {
    assert_eq!(grok::Model::Fast.as_label(), "Fast");
}
