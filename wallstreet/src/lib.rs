//! WallstreetCN financial news platform adapter.
//!
//! Two sub-adapters:
//! - live: real-time newsfeed at /live/global (categories, feed extraction, polling)
//! - general: homepage articles, search

mod models;
mod semantics;

pub use models::*;
pub use semantics::{WallstreetSemantics, LiveGlobalSemantics};
