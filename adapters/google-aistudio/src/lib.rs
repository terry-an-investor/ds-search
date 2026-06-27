//! # google-aistudio
//!
//! Google AI Studio (aistudio.google.com) adapter.
//! Supports prompt sending, response extraction, model selection, history
//! browsing, full-conversation extraction, system instructions, tool toggles,
//! temperature, thinking/reasoning extraction, and page-state inspection.

pub mod models;
pub mod semantics;

pub use models::{
    AistudioModel, AistudioState, ChatTurn, Conversation, PromptStats, ThinkingLevel, Tool,
    TurnRole,
};
pub use semantics::{AistudioSemantics, HistoryItem};
