//! Integration tests for the gemini adapter via a mock Kimi WebBridge.

use gemini::{GeminiModel, GeminiSemantics};
use pilot::KimiPrimitives;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ── Mock harness ──

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

    fn client(&self) -> GeminiSemantics {
        GeminiSemantics::new(KimiPrimitives::new(self.server.uri(), "gemini-test"))
    }
}

// ── GeminiModel enum ──

#[tokio::test]
async fn model_labels() {
    assert_eq!(GeminiModel::Fast.as_label(), "Fast");
    assert_eq!(GeminiModel::Thinking.as_label(), "Thinking");
    assert_eq!(GeminiModel::Pro.as_label(), "Pro");
}

// ── get_streaming_state ──

#[tokio::test]
async fn streaming_state_parses_json() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": json!({
                "processing": true,
                "response": "partial text",
                "thinking": "thinking content",
            }).to_string()})
        } else {
            json!({})
        }
    });
    let raw = m.client().get_streaming_state().await;
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["processing"], json!(true));
    assert_eq!(v["response"], json!("partial text"));
    assert_eq!(v["thinking"], json!("thinking content"));
}

#[tokio::test]
async fn streaming_state_empty_on_no_response() {
    let m = MockBridge::new().await;
    // Default responder returns empty object → eval returns empty → empty string
    let raw = m.client().get_streaming_state().await;
    // Should not panic; raw may be empty or unparseable, that's fine.
    assert!(raw.is_empty() || serde_json::from_str::<Value>(&raw).is_ok());
}

// ── extract_last_response ──

#[tokio::test]
async fn extract_last_response_with_text() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "Hello from Gemini!"})
        } else {
            json!({})
        }
    });
    let resp = m.client().extract_last_response().await;
    assert_eq!(resp, "Hello from Gemini!");
}

#[tokio::test]
async fn extract_last_response_empty_when_no_containers() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": ""})
        } else {
            json!({})
        }
    });
    let resp = m.client().extract_last_response().await;
    assert_eq!(resp, "");
}

// ── extract_thinking ──

#[tokio::test]
async fn extract_thinking_present() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "Show thinkingHere is my reasoning..."})
        } else {
            json!({})
        }
    });
    let thinking = m.client().extract_thinking().await;
    assert!(thinking.is_some());
    let t = thinking.unwrap();
    assert!(t.contains("reasoning"));
}

#[tokio::test]
async fn extract_thinking_none_when_empty() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": ""})
        } else {
            json!({})
        }
    });
    assert!(m.client().extract_thinking().await.is_none());
}

// ── select_model ──

#[tokio::test]
async fn select_model_finds_option() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "clicked"})
        } else {
            json!({})
        }
    });
    // Should not error when the JS returns "clicked".
    assert!(m.client().select_model(GeminiModel::Pro).await.is_ok());
}

#[tokio::test]
async fn select_model_errors_when_not_found() {
    let m = MockBridge::new().await;
    m.respond_with(|action, _| {
        if action == "evaluate" {
            json!({"value": "not found"})
        } else {
            json!({})
        }
    });
    let err = m
        .client()
        .select_model(GeminiModel::Fast)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found") || err.to_string().contains("model"));
}

// ── send_message ──

#[tokio::test]
async fn send_message_empty_rejects() {
    let m = MockBridge::new().await;
    let err = m.client().send_message("").await.unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[tokio::test]
async fn send_message_whitespace_only_rejects() {
    let m = MockBridge::new().await;
    let err = m.client().send_message("  \n\t ").await.unwrap_err();
    assert!(err.to_string().contains("empty"));
}

// ── wait_for_response ──

/// Simulates response-container count that grows then stabilizes.
#[tokio::test]
async fn wait_for_response_detects_stability() {
    let m = MockBridge::new().await;
    let state = Arc::new(Mutex::new(0u8));
    let state_clone = state.clone();
    m.respond_with(move |action, _| {
        if action == "evaluate" {
            let mut s = state_clone.lock().unwrap();
            // Sequence: 1 (start), 2 (new response appears), 2, 2, 2 (stable)
            *s = match *s {
                0 => 1,
                1 => 2,
                _ => 2,
            };
            // Wait_for_response checks both count AND processing-state-visible;
            // we return count via the first evaluate and "false" for processing.
            // Since both checks hit "evaluate", we alternate via a flag.
            json!({"value": s.to_string()})
        } else {
            json!({})
        }
    });
    let ready = m.client().wait_for_response(5).await;
    // With count growing to 2 then stable, plus processing "false", should return true.
    assert!(ready);
}

#[tokio::test]
async fn wait_for_response_times_out() {
    let m = MockBridge::new().await;
    let counter = Arc::new(Mutex::new(0u32));
    let counter_clone = counter.clone();
    m.respond_with(move |_action, _args| {
        let mut c = counter_clone.lock().unwrap();
        *c += 1;
        // Always increasing count → never stabilizes
        json!({"value": c.to_string()})
    });
    let ready = m.client().wait_for_response(1).await;
    assert!(!ready);
}
