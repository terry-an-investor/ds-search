//! Per-site command handlers.
//!
//! Each module exposes `pub async fn handle(session: String, arg: String) -> CmdResult`.
//! The registry in `main.rs` maps command names → these handlers.

pub mod aistudio;
pub mod bilibili;
pub mod deepseek;
pub mod gemini;
pub mod google;
pub mod grok;
pub mod livenews;
pub mod meta;
pub mod raw;
pub mod status;
pub mod wallstreet;
pub mod x;
