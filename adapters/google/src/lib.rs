//! # google
//!
//! Google Search adapter. Supports query, result extraction, pagination,
//! time filters, featured snippets, and AI mode.

pub mod models;
pub mod semantics;

pub use models::SearchResult;
pub use semantics::GoogleSemantics;
