//! Integration tests for the DeepSeek adapter using a mock Kimi WebBridge server.

mod mock_server;

use deepseek::{ChatMode, DeepSeekSemantics, Feature};
use mock_server::{MockKimi, browser_log_json, extract_response_json, fast_state_json};
use pilot::KimiPrimitives;

// ── Layer 1: KimiPrimitives ──

#[tokio::test]
async fn eval_js_returns_string() {
    let m = MockKimi::new().await;
    m.set_eval_response("hello", serde_json::json!("world"));
    let k = KimiPrimitives::new(m.server.uri(), "t");
    let (r, c) = k.eval_js("return 'hello'").await;
    assert_eq!(c, 0);
    assert_eq!(r, "world");
}

#[tokio::test]
async fn eval_js_handles_null() {
    let m = MockKimi::new().await;
    m.set_eval_response("nothing", serde_json::Value::Null);
    let k = KimiPrimitives::new(m.server.uri(), "t");
    let (r, c) = k.eval_js("return nothing").await;
    assert_eq!(c, 0);
    assert_eq!(r, "");
}

#[tokio::test]
async fn eval_js_handles_bool() {
    let m = MockKimi::new().await;
    m.set_eval_response("bool", serde_json::json!(true));
    let k = KimiPrimitives::new(m.server.uri(), "t");
    let (r, c) = k.eval_js("return bool").await;
    assert_eq!(c, 0);
    assert_eq!(r, "true");
}

#[tokio::test]
async fn eval_json_parses_object() {
    let m = MockKimi::new().await;
    m.set_eval_response("JSON.stringify", serde_json::json!(r#"{"has_input":true,"message_count":3}"#));
    let k = KimiPrimitives::new(m.server.uri(), "t");
    let r = k.eval_json("JSON.stringify({has_input: true, message_count: 3})").await;
    assert!(r.is_some());
    let o = r.unwrap();
    assert_eq!(o["has_input"], serde_json::json!(true));
}

#[tokio::test]
async fn eval_json_handles_invalid() {
    let m = MockKimi::new().await;
    m.set_eval_response("invalid", serde_json::json!("not json at all"));
    let k = KimiPrimitives::new(m.server.uri(), "t");
    assert!(k.eval_json("return invalid").await.is_none());
}

#[tokio::test]
async fn navigate_ok() {
    let m = MockKimi::new().await;
    let k = KimiPrimitives::new(m.server.uri(), "t");
    assert!(k.navigate("https://example.com", true).await.is_ok());
}

#[tokio::test]
async fn find_tab_not_found() {
    let m = MockKimi::new().await;
    let k = KimiPrimitives::new(m.server.uri(), "t");
    assert!(!k.find_tab("https://chat.deepseek.com").await);
}

#[tokio::test]
async fn find_tab_found() {
    let m = MockKimi::new().await;
    m.set_find_tab_found("https://chat.deepseek.com");
    let k = KimiPrimitives::new(m.server.uri(), "t");
    assert!(k.find_tab("https://chat.deepseek.com").await);
}

// ── Layer 2: DeepSeekSemantics ──

#[tokio::test]
async fn get_fast_state_ok() {
    let m = MockKimi::new().await;
    m.set_eval_response("has_input", fast_state_json(true, false, 5, "https://chat.deepseek.com", true));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    let st = s.get_fast_state().await;
    assert!(st.has_input);
    assert!(!st.is_streaming);
    assert_eq!(st.message_count, 5);
    assert!(st.has_conversation);
}

#[tokio::test]
async fn get_fast_state_defaults() {
    let m = MockKimi::new().await;
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    let st = s.get_fast_state().await;
    assert!(!st.has_input);
    assert_eq!(st.message_count, 0);
}

#[tokio::test]
async fn send_message_empty_err() {
    let m = MockKimi::new().await;
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    let r = s.send_message("").await;
    assert!(r.unwrap_err().to_string().contains("empty"));
}

#[tokio::test]
async fn send_message_no_textarea_err() {
    let m = MockKimi::new().await;
    m.set_eval_response("no-ta", serde_json::json!("no-ta"));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    let r = s.send_message("hello").await;
    assert!(r.unwrap_err().to_string().contains("textarea"));
}

#[tokio::test]
async fn select_mode_ok() {
    let m = MockKimi::new().await;
    m.set_eval_response("role", serde_json::json!("true"));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    assert!(s.select_mode(ChatMode::Expert).await);
}

#[tokio::test]
async fn select_mode_fails() {
    let m = MockKimi::new().await;
    m.set_eval_response("role", serde_json::json!("false"));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    assert!(!s.select_mode(ChatMode::Expert).await);
}

#[tokio::test]
async fn toggle_feature_ok() {
    let m = MockKimi::new().await;
    m.set_eval_response("思考", serde_json::json!("true"));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    assert!(s.toggle_feature(Feature::Thinking).await);
}

#[tokio::test]
async fn extract_last_response_ok() {
    let m = MockKimi::new().await;
    m.set_eval_response("ds-markdown", extract_response_json(true, "Hello!"));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    assert_eq!(s.extract_last_response().await, "Hello!");
}

#[tokio::test]
async fn extract_last_response_empty() {
    let m = MockKimi::new().await;
    m.set_eval_response("ds-markdown", extract_response_json(false, ""));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    assert_eq!(s.extract_last_response().await, "");
}

#[tokio::test]
async fn get_browser_log_ok() {
    let m = MockKimi::new().await;
    m.set_eval_response("window.__dsLog", browser_log_json(&[
        serde_json::json!({"lvl": "log", "t": 1000, "m": "test"}),
        serde_json::json!({"lvl": "fetch", "t": 1100, "m": "https://api.example.com"}),
    ]));
    let s = DeepSeekSemantics::new(KimiPrimitives::new(m.server.uri(), "t"));
    let log = s.get_browser_log(false).await;
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].lvl, "log");
    assert_eq!(log[1].lvl, "fetch");
}

// ── Models ──

#[tokio::test]
async fn chat_mode_labels() {
    assert_eq!(ChatMode::Quick.as_label(), "快速模式");
    assert_eq!(ChatMode::Expert.as_label(), "专家模式");
}

#[tokio::test]
async fn feature_labels() {
    assert_eq!(Feature::Thinking.as_label(), "深度思考");
    assert_eq!(Feature::Search.as_label(), "智能搜索");
}

#[tokio::test]
async fn fast_state_default() {
    let st = deepseek::FastState::default();
    assert!(!st.has_input);
    assert_eq!(st.message_count, 0);
}
