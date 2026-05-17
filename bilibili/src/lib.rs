//! Bilibili video platform adapter.
//!
//! Non-AI-chat site model: search for videos, extract results, navigate video pages.

mod models;
mod semantics;

pub use models::*;
pub use semantics::BilibiliSemantics;
