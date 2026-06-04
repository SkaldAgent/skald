# Logging & Configuration

## Logging Setup

Log files are written to `logs/` using `tracing-appender` with daily rotation:

```
logs/skald.log.YYYY-MM-DD
```

A new file is created each day. The non-blocking writer is initialized in `main()` and the `_log_guard` is kept alive for the full process lifetime to ensure all buffered logs are flushed on shutdown.

**Nothing is written to stdout/stderr** — all output goes to the log file.

---

## Log Levels and RUST_LOG

Default level: **`info`**

Override with the `RUST_LOG` env var:

| Example | Effect |
|---|---|
| `RUST_LOG=info` | Default: info and above |
| `RUST_LOG=skald=debug,info` | Debug for this crate, info for dependencies |
| `RUST_LOG=trace` | Everything (very verbose) |

Level semantics:

| Level | Use for |
|---|---|
| `ERROR` | Failures requiring a code or config fix (config load failure, DB init error, LLM loop exhausted) |
| `WARN` | Non-critical anomalies to fix eventually (malformed API response, agent skipped) |
| `INFO` | Normal significant events (server started, session opened, job executed, response tokens) |
| `DEBUG` | Per-operation lifecycle (request sent to LLM, tool dispatched, context built) |
| `TRACE` | Fine-grained internals (round counters, full request bodies, message arrays) |

A dropped WebSocket connection, a cancelled request, or a completed session are **INFO** at most — they are expected runtime events, not errors.

---

## config.yml Structure

Loaded by `Config::load()` at startup. Copied from `default.config.yaml` if `config.yml` does not exist. **Never commit `config.yml`** — it may contain API keys.

| Section | Key | Type | Default | Notes |
|---|---|---|---|---|
| `server` | `host` | string | `127.0.0.1` | Bind address |
| `server` | `port` | u16 | `3000` | HTTP/WS port |
| `web` | `static_dir` | string | `./web` | Path to static frontend files |
| `db` | `path` | string | `./database.db` | SQLite file path |
| `llm` | `max_history_messages` | usize | `30` | Max messages kept per context window. Ignored when `compaction` is configured — the compactor manages the token budget instead. |
| `llm` | `max_tool_rounds` | usize? | `20` | Max tool-call rounds per message; falls back to `DEFAULT_MAX_TOOL_ROUNDS` |

The `llm.clients` block in `config.yml` is for reference only — actual runtime LLM providers and models are stored in the DB and managed via the UI or API.

---

## LLM Providers in config.yml

| Provider | Required fields | Optional fields |
|---|---|---|
| `lm_studio` | `model` | `base_url` (default: `http://localhost:1234/v1`) |
| `ollama` | `model` | `base_url` (default: `http://localhost:11434`) |
| `openai` | `model`, `api_key` | `base_url`, `strength`, `scope` |
| `anthropic` | `model`, `api_key` | `strength`, `scope` |
| `open_ai` (OpenRouter) | `model`, `api_key`, `base_url` | `strength`, `scope`, `extra_params` |

`extra_params` is merged into the request body top-level (used for provider-specific fields like `reasoning.effort`).

---

## config.yml vs DB

| What | Where | How to change |
|---|---|---|
| Server host/port | `config.yml` | Edit file, restart app |
| DB path | `config.yml` | Edit file, restart app |
| History/round limits | `config.yml` | Edit file, restart app |
| LLM providers | DB (`llm_providers`) | UI or REST API |
| LLM models | DB (`llm_models`) | UI or REST API |
| MCP servers | DB (`mcp_servers`) | `register_mcp` tool or UI |
| Cron jobs | DB (`scheduled_jobs`) | `add_cron_job` tool or UI |

---

## When to Update This File

- `Config` struct gains or loses a field
- Log level semantics change (see also `memory/feedback_logging.md`)
- `config.yml` gains a new section or key
