# ds-search

Multi-adapter browser automation CLI for interacting with web AI platforms and search engines via [Kimi WebBridge](https://kimi.com/features/webbridge).

> **⚠️ Research only. Do not abuse.** This tool drives a real browser with your real login sessions. Rate-limiting, CAPTCHAs, account bans, and legal consequences may apply. Use responsibly and respect each platform's terms of service.

## Prerequisites

- [Kimi WebBridge](https://kimi.com/features/webbridge) browser extension installed and running at `http://127.0.0.1:10086`
- [Rust](https://rustup.rs/) (edition 2024)

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
| `deepseek` | chat.deepseek.com | Send prompts, extract responses/thinking, toggle search/thinking mode, multi-turn extraction, open history sessions, new conversations |
| `grok` | x.com/i/grok | Send prompts, extract responses, new conversations |
| `gemini` | gemini.google.com | Send prompts, extract responses/thinking, select model (Fast/Thinking/Pro), stream detection |
| `bilibili` | bilibili.com | Search videos, extract results (title/duration/uploader), pagination, sort, video details |
| `wallstreet` | wallstreetcn.com | Extract articles, search, article body extraction |
| `livenews` | wallstreetcn.com/live/global | Extract live news items, filter by category, important-only toggle, polling |
| `google` | google.com | Web/image/video/news/shopping/forums/books/AI search, pagination, time filters, snippets |
| `aistudio` | aistudio.google.com | Send prompts, extract responses/conversation, select model, thinking level, system instructions, tool toggles, temperature, get API code |
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
ds deepseek ask "Explain Rust ownership"   # send + wait-for-stable + extract (atomic)
ds deepseek extract                          # latest model reply only
ds deepseek turns                            # full multi-turn conversation
ds deepseek open "Rust ownership chat"       # open a history session by title (or /a/chat/s/<id> URL)
ds deepseek thinking
ds deepseek toggle search
ds deepseek mode expert

# Google Search
ds google search "Rust async traits"
ds google search images cute cats
ds google search news climate change
ds google next
ds google recent "rust lang" h

# AI Studio (run `ds aistudio` with no args to list all subcommands)
ds aistudio ask "Write a Python hello world"   # send + wait + extract (atomic, auto-reruns on failure)
ds aistudio model pro
ds aistudio thinking low
ds aistudio system "Be concise"
ds aistudio tool search                          # toggle Grounding with Google Search
ds aistudio turns                                # full conversation
ds aistudio code                                 # get API code for the current prompt

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

`meta save-structure` stores only stable fields (url/title/inputs/buttons), so the file is
reproducible and safe to commit as a baseline for detecting site redesigns via `meta diff`.
Use it on low-churn sites (`deepseek`, `grok`, `gemini`, `aistudio`); avoid it on time-sensitive
ones (`livenews`, `wallstreet`) whose body text changes every refresh — use the full `meta save`
locally for those.

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
4. Wire up the CLI handler:
   - Create `cli/src/handlers/<name>.rs` exposing `pub async fn handle(session, arg) -> CmdResult`
   - Declare it in `cli/src/handlers/mod.rs`
   - Register it in `registry()` in `cli/src/main.rs`
5. Build with `cargo build` and test against the live site

## Troubleshooting

- **`ds raw url` returns HTTP 502 while `ds status` says connected** — WebBridge's `evaluate`
  channel needs at least one open browser tab. Run `ds raw navigate https://example.com` once
  to activate it.

## License

MIT
