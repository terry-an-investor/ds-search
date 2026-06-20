//! Shared CLI types and helpers used by all command handlers.

use pilot::KimiPrimitives;
use std::future::Future;
use std::pin::Pin;

/// Result type for all command handlers.
pub type CmdResult = Result<String, Box<dyn std::error::Error>>;

/// Pinned boxed future returned by command handlers (async fn wrapped in registry).
pub type CmdFuture = Pin<Box<dyn Future<Output = CmdResult> + Send>>;

/// A registered command handler: takes (session, arg) and returns a CmdFuture.
pub type Handler = Box<dyn Fn(String, String) -> CmdFuture + Send + Sync>;

/// The Kimi WebBridge HTTP endpoint.
const KIMI_BASE_URL: &str = "http://127.0.0.1:10086";

/// Build a KimiPrimitives client bound to the given session name.
pub fn kimi(session: &str) -> KimiPrimitives {
    KimiPrimitives::new(KIMI_BASE_URL, session)
}

/// Split an arg string into (first_token, rest) on the first space.
pub fn split_arg(arg: &str) -> (&str, &str) {
    let mut parts = arg.splitn(2, ' ');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

/// Truncate a string to `max` bytes on a UTF-8 char boundary.
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
