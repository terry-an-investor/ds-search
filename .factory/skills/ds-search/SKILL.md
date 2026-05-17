---
name: ds-search
description: |
  Query DeepSeek Chat (chat.deepseek.com) through Kimi WebBridge.
  Use when the user wants to ask DeepSeek a question, search the web,
  or get AI responses with deep thinking.
---

# ds-search — Query DeepSeek Chat

One command does everything:

```bash
./target/release/ds send "your question here"
```

This handles page setup, sending, streaming wait, and response extraction atomically.

## When the user asks for thinking or web search

Toggle before sending (these check current DOM state, won't double-toggle):

```bash
./target/release/ds toggle thinking   # 深度思考 ON/OFF
./target/release/ds toggle search     # 智能搜索 ON/OFF
```

## Starting fresh

```bash
./target/release/ds new               # new conversation, then:
./target/release/ds send "question"
```

## Debugging failures

If a send fails, check page state and retry:

```bash
./target/release/ds state             # fast state: streaming?, messages?, url
RUST_LOG=ds_adapter=debug ./target/release/ds send "question"
```

## Rules

- Run `ds send` sequentially — never in parallel.
- Toggle thinking/search only when the user explicitly asks.
- If `ds send` fails with "No response extracted", wait 5s and retry once. If it fails again, report the error.
