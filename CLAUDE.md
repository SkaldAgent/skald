
# Skald — codebase guide

Rust async web app (Tokio + Axum). Runs as a local chat server with LLM tool-calling, a sub-agent system, and the ability to rewrite and restart itself.

## Key modules

| Path | Role |
| ---- | ---- |
| `src/main.rs` | Thin entry point: tracing → `Skald::new` → `WebFrontend::start` → shutdown |
| `src/core/skald.rs` | `Skald` — headless application core; owns all managers; `new()` / `shutdown()` |
| `src/core/session/handler/` | Core LLM loop — `mod.rs`, `llm_loop.rs` (`run_agent_turn`), `agent_dispatch.rs`, `dispatcher.rs`, `approval.rs`, `resume.rs`, `messages.rs`, `config.rs`, `interface_tools.rs` |
| `src/core/session/manager.rs` | Creates/retrieves `ChatSessionHandler` per session |
| `src/core/chat_hub/` | `ChatHub`: broadcast events to all connected WS clients |
| `src/core/chat_event_bus.rs` | Global async bus for cross-session events |
| `src/core/agents.rs` | Discovers agents from `agents/*/`, loads meta + system prompt |
| `src/core/tools/` | Built-in tools: `exec`, `restart`, `list_agents`, `fs/*`, `notify`, `ast_outline`, `image_generate`, MCP tools, plugin tools, cron tools |
| `src/core/tool_catalog.rs` | `ToolCatalog`: unified tool listing façade (wraps ToolRegistry + McpManager) |
| `src/core/events.rs` | `ServerEvent` enum streamed over WebSocket to the frontend |
| `src/core/db/` | sqlx SQLite — see below |
| `src/config.rs` | Loads `config.yml`; LLM clients, strength/use_cases, data root |
| `src/core/mcp/` | MCP client manager (connects to external MCP servers) |
| `src/core/plugin/` | Plugin system: discovery, enable/disable, tool registration |
| `src/core/cron/` | Scheduled job runner |
| `src/core/compactor.rs` | Context compaction (summarises history when token budget exceeded) |
| `src/core/approval/` | Approval rules engine |
| `src/core/clarification/` | `ClarificationManager`: background-session question/answer |
| `src/core/inbox.rs` | `Inbox`: unified façade for pending approvals + clarifications (wraps ApprovalManager, ClarificationManager, ChatHub) |
| `src/core/llm/` | LLM client abstraction (OpenAI-compat, Anthropic, Ollama…) |
| `src/core/transcribe/` | Transcription providers |
| `src/core/image_generate/` | Image generation providers |
| `src/core/memory/` | Agent memory tools |
| `src/frontend/mod.rs` | `WebFrontend`: wires router_factory, starts plugins, runs Axum |
| `src/frontend/server.rs` | Axum router, static file serving |
| `src/frontend/api/` | HTTP + WebSocket handlers — `State<Arc<Skald>>` |
| `web/components/` | Lit web components (see below) |

## DB tables (sqlx SQLite)

`chat_sessions`, `chat_sessions_stack`, `chat_history`, `chat_llm_tools`, `chat_summaries`, `llm_requests`, `scheduled_jobs`, `job_runs`, `mcp_servers`, `mcp_events`, `plugins`, `approval_rules`, `sources`, `scratchpad`, `session_mcp_grants`, `stack_mcp_grants`

## Sub-agent system

- Synchronous sub-agents (`execute_task` mode=sync / `run_subtask`) are **not** plain `Tool`s — they are intercepted in `run_agent_turn` before registry dispatch.
- `dispatch_sub_agent` (in `agent_dispatch.rs`) creates a child `chat_sessions_stack` row and runs `run_agent_turn` **recursively in the same task**, holding the same `processing` lock and sharing the same cancellation token. The child's result string becomes the parent tool call's result (completion lives in one place — the `run_agent_turn` tool-result match); then it terminates the child frame. There is no task-spawn / `WaitingChild` / resume cascade for the sync path.
- Max recursion depth: `MAX_AGENT_DEPTH = 5`.
- Client resolution order: `args.client` → `meta.json client` → AUTO selection by scope/strength.
- **The parent's resolved client is NOT inherited.** Passing a concrete model name to `resolve()` bypasses strength/scope checks; sub-agents always auto-select unless overridden explicitly.
- `list_agents` is a plain tool; returns JSON excluding `main`.
- `resume_turn` (+ its cascade) is kept only for: app-restart recovery of an active child stack, async task result injection (`inject_async_result`), and the WS resume message — not for the normal sync dispatch.

## Cancellation (stop)

- Each turn has a `CancellationToken` (`tokio_util`). `handle_message` mints a fresh one per user message and stores it in `current_cancel`; `resume_turn` mints one per resume. A **clone is threaded by value** through the whole (recursive) call tree — never re-read from the field mid-turn — so a `/stop` is **sticky** across sub-agent recursion.
- `cancel()` cancels the stored token. It is checked at each round boundary and before each tool call, wrapped around the in-flight LLM call (`tokio::select!`, aborting the request), and wrapped around `execute_cmd` (drops the future → `kill_on_drop` kills the shell process). Parent and child share the token, so a cancelled child stops the parent by construction.

## Approval gate

`needs_approval()` returns true for `execute_cmd`, `restart`, and any write-file tool targeting paths outside `memory/`. Approval is a `oneshot` channel registered in `approval_registry`; resolved via `resolve_approval()` from the WS handler.

## Self-restart

`restart` tool calls `std::process::exit(-1)` (= exit code 255). `run.sh` supervisor loop: exit 255 → rebuild+restart, exit 0 → stop clean.

## Build & run

```sh
cargo build
./run.sh        # supervisor loop (rebuilds on exit -1)
```

Tracing filter: `RUST_LOG=skald=debug,info`

## Adding an agent

Create `agents/<id>/meta.json` and `agents/<id>/AGENT.md`. The agent is discovered at runtime (no restart needed for prompt edits). Optionally set `"client": "<name>"` in meta.json to pin a specific LLM.

## Documentation

Project documentation lives in `docs/`. **Always update `docs/` alongside any code change** — never leave it outdated. The main entry point is `docs/index.md`; each subsystem has its own file (e.g. `docs/frontend.md`, `docs/session.md`, `docs/tools.md`).

## Config

Copy `default.config.yaml` → `config.yml`. Never commit `config.yml` (contains API keys).

## Python environment

All Python scripts (MCP servers, setup scripts) use a local virtualenv at `.venv/` in the project root.

`run.sh` creates it automatically on first launch (using `uv` if available, otherwise `python3 -m venv`) and installs `requirements.txt`. It then prepends `.venv/bin` to `PATH` before starting the app, so every child process — MCP server launches, `execute_cmd` shell calls — resolves `python3` to the venv automatically. No manual activation needed. **Python is optional**: if neither `uv` nor `python3` is found, the app starts normally and only Python-based MCP servers will be unavailable.

To add a Python dependency: add it to `requirements.txt`. It will be installed on the next `./run.sh` invocation if `.venv` does not yet exist — or run `uv pip install -r requirements.txt` manually.

## Frontend components (`web/components/`)

All extend `LightElement` from `web/lib/base.js` (Lit). `ChatSession` (`web/lib/chat-session.js`) is the shared base for WS-connected chat UIs.

| File | Element | Notes |
| ---- | ------- | ----- |
| `copilot.js` | `<app-copilot>` | Desktop copilot (`_wsSource='web'`); composer input with model pill, auto-resize textarea |
| `shared/chat-page.js` | `<chat-page>` | Mobile chat (`_wsSource='mobile'`) |
| `copilot-render.js` | (helpers) | `renderMsg`, `renderTool`, `renderDiff`, etc. — shared by copilot and chat-page |
| `sidebar.js` | `<app-sidebar>` | Nav sidebar; polls `/api/inbox` every 10 s for badge |
| `topbar.js` | `<app-topbar>` | Top nav bar |
| `editor.js` | — | File editor panel |
| `home-page.js` | `<home-page>` | Landing / dashboard |
| `agents.js` | `<agents-page>` | Agent discovery and config |
| `agent-inbox.js` | `<agent-inbox-page>` | Pending approvals + clarifications from background sessions |
| `approval-rules.js` | `<approval-rules-page>` | Approval rule management |
| `cron-jobs.js` | `<cron-jobs-page>` | Scheduled job management |
| `llm-providers.js` | `<llm-providers-page>` | LLM provider management |
| `models-hub.js` | `<models-hub-page>` | Models hub landing (LLM / Transcription / Image) |
| `models-llm.js` | `<models-llm-section>` | LLM model CRUD + drag-and-drop priority |
| `models-transcribe.js` | `<models-transcribe-section>` | Transcription model CRUD |
| `models-image.js` | `<models-image-section>` | Image generation model CRUD |
| `mobile-app.js` | `<mobile-app>` | Mobile app shell |
