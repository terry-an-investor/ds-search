//! Mock Kimi WebBridge server for integration tests.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub struct MockKimi {
    pub server: MockServer,
    pub eval_responses: Arc<Mutex<HashMap<String, Value>>>,
    pub find_tab_found: Arc<Mutex<HashMap<String, bool>>>,
}

impl MockKimi {
    pub async fn new() -> Self {
        let server = MockServer::start().await;
        let eval_responses = Arc::new(Mutex::new(HashMap::<String, Value>::new()));
        let find_tab_found = Arc::new(Mutex::new(HashMap::<String, bool>::new()));

        let eval_map = eval_responses.clone();
        let find_map = find_tab_found.clone();
        Mock::given(method("POST"))
            .and(path("/command"))
            .respond_with(move |req: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or_default();
                let action = body["action"].as_str().unwrap_or("");
                let args = &body["args"];
                let eval_map = eval_map.lock().unwrap();
                let find_map = find_map.lock().unwrap();

                let data = match action {
                    "evaluate" => {
                        let code = args["code"].as_str().unwrap_or("");
                        let result = if let Some(v) = eval_map.get(code) {
                            v.clone()
                        } else {
                            eval_map.iter().find(|(k, _)| code.contains(k.as_str()))
                                .map(|(_, v)| v.clone()).unwrap_or(Value::Null)
                        };
                        serde_json::json!({"value": result})
                    }
                    "navigate" => serde_json::json!({"success": true}),
                    "find_tab" => {
                        let url = args["url"].as_str().unwrap_or("");
                        let success = find_map.get(url).copied().unwrap_or(false);
                        serde_json::json!({"success": success})
                    }
                    "list_tabs" => serde_json::json!({"tabs": []}),
                    "close_tab" => serde_json::json!({"success": true}),
                    _ => serde_json::json!({}),
                };
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true, "data": data}))
            })
            .mount(&server)
            .await;

        Self { server, eval_responses, find_tab_found }
    }

    pub fn set_eval_response(&self, script_pattern: &str, value: Value) {
        let mut map = self.eval_responses.lock().unwrap();
        map.insert(script_pattern.to_string(), value);
    }

    pub fn set_find_tab_found(&self, url: &str) {
        let mut map = self.find_tab_found.lock().unwrap();
        map.insert(url.to_string(), true);
    }
}

pub fn fast_state_json(
    has_input: bool, is_streaming: bool, message_count: usize,
    url: &str, has_conversation: bool,
) -> Value {
    Value::String(serde_json::json!({
        "has_input": has_input, "is_streaming": is_streaming,
        "message_count": message_count, "url": url,
        "has_conversation": has_conversation,
        "title": "DeepSeek"
    }).to_string())
}

pub fn extract_response_json(exists: bool, content: &str) -> Value {
    Value::String(serde_json::json!({"exists": exists, "content": content}).to_string())
}

pub fn browser_log_json(entries: &[Value]) -> Value {
    Value::String(serde_json::json!({"entries": entries, "count": entries.len()}).to_string())
}
