//! `status` — probe whether Kimi WebBridge is reachable.

use crate::types::CmdResult;

pub async fn handle(session: String, _arg: String) -> CmdResult {
    let client = reqwest::Client::new();
    match client
        .post("http://127.0.0.1:10086/command")
        .json(&serde_json::json!({"action": "list_tabs", "args": {}, "session": session}))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Ok("connected".into()),
        Ok(resp) => Err(format!("HTTP {}", resp.status()).into()),
        Err(e) => Err(format!("not connected: {}", e).into()),
    }
}
