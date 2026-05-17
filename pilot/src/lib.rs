//! # pilot
//!
//! Site-agnostic browser automation primitives via Kimi WebBridge HTTP API.
//! Works with any webpage — no site-specific knowledge.

pub mod error;
pub mod kimi;
pub mod logging;
pub mod models;

pub use error::{AdapterError, Result};
pub use kimi::KimiPrimitives;
pub use logging::init_logging;
pub use models::BrowserLogEntry;
