//! # google-aistudio
//!
//! Google AI Studio (aistudio.google.com) adapter.
//! Supports prompt sending, response extraction, model selection, and history browsing.

pub mod models;
pub mod semantics;

pub use models::AistudioModel;
pub use semantics::AistudioSemantics;
