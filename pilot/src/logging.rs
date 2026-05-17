use std::io::IsTerminal;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

/// Initialize structured logging.
///
/// By default, all `ds_adapter` events at INFO+ are emitted.
/// Set `RUST_LOG=ds_adapter=debug` to include params and return values.
/// Set `RUST_LOG=ds_adapter=trace` to also include internal spans.
/// Set `RUST_LOG_JSON=1` to emit JSON lines instead of human-readable output.
pub fn init_logging() {
    let use_json = std::env::var("RUST_LOG_JSON")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let default_directive = "ds_adapter=info";
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_directive))
        .unwrap();

    if use_json {
        tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true)
            .with_env_filter(env_filter)
            .with_target(false)
            .init();
    } else {
        let use_color = std::io::stderr().is_terminal();
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .with_ansi(use_color)
            .with_span_events(FmtSpan::NONE)
            .init();
    }

    tracing::info!(
        mode = if use_json { "json" } else { "text" },
        "logging initialized"
    );
}

/// Dump the last N browser log entries at debug level.
pub fn log_browser_entries(entries: &[crate::models::BrowserLogEntry], max: usize) {
    if entries.is_empty() {
        tracing::debug!("browser log: (empty)");
        return;
    }
    tracing::debug!("browser log (last {} of {}):", max.min(entries.len()), entries.len());
    for entry in entries.iter().rev().take(max).rev() {
        tracing::debug!(
            lvl = %entry.lvl,
            time = %entry.t,
            msg = %entry.m,
        );
    }
}
