//! # weread
//!
//! WeRead (weread.qq.com) digital reading platform adapter.
//! Supports search, book info, chapter listing, and reading.

pub mod models;
pub mod semantics;

pub use models::{BookInfo, ChapterInfo};
pub use semantics::WeReadSemantics;
