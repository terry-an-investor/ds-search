# ds-search

Multi-adapter browser automation CLI for interacting with web AI platforms and search engines via [Kimi WebBridge](https://kimi.com/features/webbridge).

> **⚠️ Research only. Do not abuse.** This tool drives a real browser with your real login sessions. Rate-limiting, CAPTCHAs, account bans, and legal consequences may apply. Use responsibly and respect each platform's terms of service.

## Prerequisites

- [Kimi WebBridge](https://kimi.com/features/webbridge) browser extension installed and running at `http://127.0.0.1:10086`
- [Rust](https://rustup.rs/) (edition 2024)
- **An active browser tab.** WebBridge's `evaluate` channel requires at least one open tab; commands return `HTTP 502` when the browser has no tabs. Run `ds raw navigate https://example.com` once to activate it if `ds status` says connected but `ds raw url` returns 502.

## Quick Start

```bash
# Build
cargo build

# Check if WebBridge is connected
cargo run -- status

# Scan current page for interactive elements
cargo run -- meta scan
```

## Supported Platforms

| Command | Site | Capabilities |
|---------|------|-------------|
| `deepseek` | chat.deepseek.com | Send prompts, extract responses/thinking, toggle search/thinking mode, new conversations |
| `grok` | x.com/i/grok | Send prompts, extract responses, new conversations |
| `gemini` | gemini.google.com | Send prompts, extract responses/thinking, select model (Fast/Thinking/Pro), stream detection |
| `bilibili` | bilibili.com | Search videos, extract results (title/duration/uploader), pagination, sort, video details |
| `wallstreet` | wallstreetcn.com | Extract articles, search, article body extraction |
| `livenews` | wallstreetcn.com/live/global | Extract live news items, filter by category, important-only toggle, polling |
| `google` | google.com | Web/image/video/news/shopping/forums/books/AI search, pagination, time filters, snippets |
| `aistudio` | aistudio.google.com | Send prompts, extract responses, select model, set thinking level, browse history, get API code |
| `x` | x.com | Extract tweet threads (main tweet + self-replies), external links, engagement stats |

## Usage

All commands follow the pattern:

```bash
# Basic
ds <command> <subcommand> [args...]

# With specific session (isolated browser tab group)
ds --session <name> <command> <subcommand> [args...]
```

### Examples

> Most site commands auto-navigate to the target via `ensure` (e.g. `ds deepseek send ...`).
> A few read-only commands (`wallstreet articles`, `livenews items`) assume the page is already
> loaded by that adapter — run `ds <site> ensure` first if you get empty results after switching
> to a different site's tab.

```bash
# DeepSeek
ds deepseek send "Explain Rust ownership"
ds deepseek ask "Explain Rust ownership"   # send + wait-for-stable + extract (atomic)
ds deepseek extract
ds deepseek thinking
ds deepseek toggle search
ds deepseek mode expert

# Google Search
ds google search "Rust async traits"
ds google search images cute cats
ds google search news climate change
ds google next
ds google recent "rust lang" h

# AI Studio
ds aistudio send "Write a Python hello world"
ds aistudio wait
ds aistudio extract
ds aistudio model pro
ds aistudio thinking low
ds aistudio history 10
ds aistudio code

# Bilibili
ds bilibili search "Rust tutorial"
ds bilibili results 10
ds bilibili sort newest

# X (Twitter)
ds x thread <tweet_url>
ds x links <tweet_url>

# Meta (page inspection)
ds meta scan
ds meta save my-page                      # full snapshot (debugging: includes body text + timestamp)
ds meta save-structure my-baseline        # structural snapshot (regression baseline: url/title/inputs/buttons only)
ds meta diff my-baseline                  # detect DOM drift since the snapshot
ds meta watch 500x20
```

### Structural snapshots for regression baselines

`meta save-structure` stores only stable fields (url, title, inputs' placeholder/disabled,
buttons' text/role/checked) — no `timestamp`, `bodySnippet`, or `dynEls`. This makes the
file reproducible across repeated saves (zero diff when the page hasn't changed), so it is
safe to commit to version control as a regression baseline for detecting site redesigns.

**Suitable for tracking** (low churn, stable structure): `deepseek`, `grok`, `gemini`,
`aistudio`. When one of these ships a redesign that renames a selector, `meta diff <name>`
will flag the added/removed buttons so you know which adapter to fix.

**Never track** (time-sensitive content): `livenews`, `wallstreet` — their DOM is correct but
the body text changes every refresh; use the full `meta save` locally for debugging those.

## Project Structure

```
.
├── pilot/              # Core library — Kimi WebBridge HTTP client + error types
├── cli/                # CLI binary — command dispatch + registry
│   ├── src/main.rs     # Entry point + command registry (one line per command)
│   ├── src/types.rs    # Shared CmdResult / Handler / kimi() / split_arg() helpers
│   └── src/handlers/   # One file per command (deepseek.rs, google.rs, meta.rs, ...)
├── adapters/           # Site-specific adapter crates
│   ├── deepseek/       # DeepSeek Chat (semantics + models)
│   ├── grok/           # Grok
│   ├── gemini/         # Google Gemini
│   ├── bilibili/       # Bilibili
│   ├── wallstreet/     # Wallstreetcn (general + live/global)
│   ├── google/         # Google Search
│   ├── google-aistudio/# Google AI Studio
│   └── x/              # X.com (tweet threads)
├── knowledge/          # Site structure docs (YAML) + DOM baselines (scans/*.json)
└── Makefile            # Build, test, and cleanup targets
```

## Build & Cleanup

```bash
make build     # Build + sweep intermediate artifacts
make test      # Run tests
make sweep     # Remove intermediate build artifacts (keeps binary)
make clean     # Full clean (removes target/)
make clean-system  # Clean ~/.cargo cache (stale crate sources)
```

## Adding a New Site

1. Create `knowledge/<domain>.yaml` documenting the site's DOM structure, APIs, and pitfalls
2. Create `adapters/<name>/` with `Cargo.toml`, `src/lib.rs`, `src/models.rs`, `src/semantics.rs`
3. Add the crate to `Cargo.toml` workspace members and `cli/Cargo.toml` dependencies
4. Create `cli/src/handlers/<name>.rs` exposing `pub async fn handle(session, arg) -> CmdResult`, declare it in `cli/src/handlers/mod.rs`, and register it in `registry()` in `cli/src/main.rs`
5. Build with `cargo build` and test against the live site

## License

MIT
