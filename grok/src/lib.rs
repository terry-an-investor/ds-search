//! # grok
//!
//! Grok adapter for interacting with [x.com/i/grok](https://x.com/i/grok)
//! via [Kimi WebBridge](https://github.com/nicepkg/kimi-web-bridge).
//!
//! | Layer | Module | Responsibility |
//! |-------|--------|----------------|
//! | L1 | `pilot::kimi::KimiPrimitives` | Generic browser operations (shared) |
//! | L2 | [`semantics`] | Grok-specific page actions (send, extract, toggle) |
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use grok::GrokSemantics;
//! use pilot::KimiPrimitives;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let kimi = KimiPrimitives::new("http://127.0.0.1:10086", "grok");
//!     let grok = GrokSemantics::new(kimi);
//!
//!     grok.ensure_tab().await?;
//!     grok.send_message("hello").await?;
//!     let response = grok.extract_last_response().await;
//!     println!("{}", response);
//!     Ok(())
//! }
//! ```

pub mod models;
pub mod semantics;

pub use models::Model;
pub use semantics::GrokSemantics;
