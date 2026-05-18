# ds-search

Multi-adapter browser automation CLI for interacting with web AI platforms and search engines via [Kimi WebBridge](https://kimi.com/features/webbridge).

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
| `deepseek` | chat.deepseek.com | Send prompts, extract responses/thinking, toggle search/thinking mode, new conversations |
| `grok` | grok.com | Send prompts, extract responses, new conversations |
| `gemini` | gemini.google.com | Send prompts, extract responses/thinking, select model (Fast/Thinking/Pro), stream detection |
| `bilibili` | bilibili.com | Search videos, extract results (title/duration/uploader), pagination, sort, video details |
| `wallstreet` | wallstreetcn.com | Extract articles, search, article body extraction |
| `livenews` | wallstreetcn.com/live/global | Extract live news items, filter by category, important-only toggle, polling |
| `weread` | weread.qq.com | Search books, open books, get book info/chapters, read page text, AI outlines, highlights |
| `google` | google.com | Web/image/video/news/shopping/forums/books/AI search, pagination, time filters, snippets |
| `aistudio` | aistudio.google.com | Send prompts, extract responses, select model, set thinking level, browse history, get API code |

## Usage

All commands follow the pattern:

```bash
# Basic
ds <command> <subcommand> [args...]

# With specific session (isolated browser tab group)
ds --session <name> <command> <subcommand> [args...]
```

### Examples

```bash
# DeepSeek
ds deepseek send "Explain Rust ownership"
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

# WeRead
ds weread search "三体"
ds weread open <url>
ds weread chapters
ds weread highlights

# Meta (page inspection)
ds meta scan
ds meta save my-page
ds meta diff my-page
ds meta watch 500x20
```

## Project Structure

```
.
├── pilot/              # Core library — Kimi WebBridge HTTP client + error types
├── cli/                # CLI binary — command registry and handlers
├── adapters/           # Site-specific adapter crates
│   ├── deepseek/       # DeepSeek Chat
│   ├── grok/           # Grok
│   ├── gemini/         # Google Gemini
│   ├── bilibili/       # Bilibili
│   ├── wallstreet/     # Wallstreetcn (general + live/global)
│   ├── weread/         # WeRead
│   ├── google/         # Google Search
│   └── google-aistudio/# Google AI Studio
├── knowledge/          # Site structure documentation (YAML)
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
4. Add a `handle_<name>()` async function and register it in `registry()` in `cli/src/main.rs`
5. Build with `cargo build` and test against the live site

## License

MIT
