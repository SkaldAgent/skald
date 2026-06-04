# Architecture

## Component Map

| Struct | Created by | Held as | Depends on |
| --- | --- | --- | --- |
| `SqlitePool` | `db::init_pool()` | `Arc<SqlitePool>` | — |
| `LlmManager` | `LlmManager::new()` | `Arc<LlmManager>` | `SqlitePool` |
| `McpManager` | `McpManager::new()` | `Arc<McpManager>` | `SqlitePool` |
| `CronTaskManager` | `CronTaskManager::new()` | `Arc<CronTaskManager>` | `SqlitePool`, `ChatSessionManager` (via OnceLock), `ChatHub` (via OnceLock) |
| `ToolRegistry` | `main.rs` inline | `Arc<ToolRegistry>` | `McpManager`, `CronTaskManager`, `PluginManager` |
| `ApprovalManager` | `ApprovalManager::new()` | `Arc<ApprovalManager>` | `SqlitePool` |
| `ClarificationManager` | `ClarificationManager::new()` | `Arc<ClarificationManager>` | — |
| `ChatEventBus` | `ChatEventBus::new()` | `Arc<ChatEventBus>` | — |
| `ContextCompactor` | `main.rs` (when `llm.compaction` configured) | `Option<Arc<ContextCompactor>>` | `LlmManager`, `ChatEventBus` |
| `ChatSessionManager` | `ChatSessionManager::new()` | `Arc<ChatSessionManager>` | `SqlitePool`, `LlmManager`, `ToolRegistry`, `McpManager`, `ApprovalManager`, `ClarificationManager`, `ChatEventBus`, `ContextCompactor` |
| `ChatHub` | `ChatHub::new()` | `Arc<ChatHub>` | `SqlitePool`, `ChatSessionManager`, `ApprovalManager` |
| `TicManager` | `TicManager::new()` | `Arc<TicManager>` | `SqlitePool`, `ChatHub`, `ChatSessionManager` |
| `AppState` | `main.rs` inline | cloned into Axum router | all of the above |

### Circular Dependencies

**`CronTaskManager` ↔ `ChatSessionManager`**: `CronTaskManager` needs `ChatSessionManager` to dispatch jobs, but `ChatSessionManager` is built after `ToolRegistry` which holds `Arc<CronTaskManager>`. Broken with `std::sync::OnceLock`: `CronTaskManager` is created first, `set_session()` is called after `ChatSessionManager` exists.

**`CronTaskManager` ↔ `ChatHub`**: Same pattern — `ChatHub` is built after `cron.start()`. `set_hub()` is called immediately after `ChatHub::new()`. The cron tick loop starts 30 s after `start()`, so hub is always ready by the first real job dispatch.

---

## Startup Sequence

1. Init logging (`tracing-appender` daily rolling to `logs/`)
2. `Config::load()` — reads `config.yml` (copies from `default.config.yaml` if missing)
3. `db::init_pool()` — opens SQLite, runs `create_tables()` (idempotent)
4. `agents::discover()` — scans `agents/*/` for `meta.json` + `AGENT.md`
5. `LlmManager::new()` — loads providers and models from DB
6. `McpManager::new()` + background `initialize()` — connects MCP servers from DB; starts `notification_consumer` task persisting MCP push events to `mcp_events`
7. `CronTaskManager::new()` — creates scheduler (not started yet)
8. `PluginManager` built — plugins registered, not yet started
9. `ToolRegistry` built — all built-in tools registered (`notify` is **not** in the registry — see tools.md)
10. `ApprovalManager::new()` — loads approval rules from DB
11. `ImageGeneratorManager::new(pool, "data")` — image generation provider registry; loads DB-backed models
12. `ChatEventBus::new()` — in-process broadcast bus for chat events (no subscribers at startup)
13. `ClarificationManager::new()` — in-memory pending clarification store for background sessions
14. `ChatSessionManager::new()` — session factory wired up; receives `ClarificationManager` and `ImageGeneratorManager`
15. `cron.set_session()` — breaks CronTaskManager circular dep
16. `cron.start()` — background scheduler loop begins (tick every 30 s); recovery of interrupted jobs runs once before the first tick; cleanup loop starts (15 s delay then hourly)
17. `TranscribeManager::new()` — STT provider registry
18. `ChatHub::new()` — central chat orchestrator; spawns notification consumer task
19. `cron.set_hub(chat_hub)` — wires ChatHub into CronTaskManager for completion notifications
20. `TicManager::new(pool, session_mgr, chat_hub, config.tic)` + `.start()` — background MCP event processor; `Arc<ChatHub>` passed directly (no OnceLock needed — TicManager is created after ChatHub)
21. `AppState` assembled
22. `PluginManager::set_state()` + `start_enabled()` — starts Telegram and other enabled plugins
23. `WebServer::start()` — Axum HTTP+WS server starts listening

---

## Request Lifecycle

1. Client opens WebSocket: `GET /api/ws`
2. `handle_socket()` gets or creates `ChatSessionHandler` via `ChatHub::session_handler("web")`
3. Client sends `ClientMessage` JSON over WS
4. `ChatHub::send_message("web", prompt, opts)` is called
5. Handler spawns async task: `handler.handle_message(...)`
6. `handle_message` acquires `processing: Mutex<()>` (one at a time per session)
7. `run_agent_turn` loop starts (up to `max_tool_rounds` rounds):
   - Build context: `build_openai_messages()` → system prompt + history + tool results
   - Apply `allow_tools` filter (if agent has whitelist in `meta.json`)
   - Call LLM: `llm.client.chat_with_tools()`
   - LLM returns `LlmTurn::Message` → send `Done` event, exit loop
   - LLM returns `LlmTurn::ToolCalls` → for each call:
     - Approval check → optionally send `PendingWrite`, wait for user
     - Dispatch tool → send `ToolStart` / `ToolDone` / `ToolError`
     - `call_agent` → recurse via `dispatch_call_agent`
8. Main loop sends `Done` event with final content and token counts

---

## Notification Flow (background)

```text
MCP server stdout (JSON-RPC notification, no id field)
  → McpServer reader loop (src/mcp/server.rs)
  → notification_tx (mpsc::UnboundedSender)
  → McpManager::notification_consumer
  → db::mcp_events::insert(source, method, payload)

[every tic.interval_secs (default 900 s) — TicManager::run_tick()]
  → mcp_events::pending_limited(tic.batch_size)
  → mcp_events::mark_processed(ids)
  → build_prompt(events)
  → ChatHub::send_message("tic", prompt)   ← ephemeral session
  → TIC agent runs, calls notify(briefing)
  → ChatHub::notify_sync → mpsc channel

[ChatHub::notification_consumer]
  → batching window (200 ms)
  → send_message(home_source, "[SYSTEM - NOTIFICATION]\n...")
  → user sees assistant briefing in home conversation
```

---

## AppState Fields

| Field | Type | Purpose |
| --- | --- | --- |
| `manager` | `Arc<ChatSessionManager>` | Creates/retrieves session handlers |
| `chat_hub` | `Arc<ChatHub>` | Central chat orchestrator; routes messages, notifications, approvals |
| `db` | `Arc<SqlitePool>` | Direct DB access for API routes |
| `mcp` | `Arc<McpManager>` | MCP server management API |
| `cron` | `Arc<CronTaskManager>` | Cron job management API |
| `plugin_manager` | `Arc<PluginManager>` | Plugin lifecycle management |
| `location_manager` | `Arc<LocationManager>` | Named GPS position store |
| `approval` | `Arc<ApprovalManager>` | Human-in-the-loop approval rules |
| `clarification` | `Arc<ClarificationManager>` | Pending clarification requests from background sessions (Agent Inbox) |
| `tools` | `Arc<ToolRegistry>` | Built-in tool dispatch |
| `transcribe_manager` | `Arc<TranscribeManager>` | Speech-to-Text provider registry |
| `image_generator_manager` | `Arc<ImageGeneratorManager>` | Text-to-image provider registry (DB-backed + plugin) |
| `memory_manager` | `Arc<MemoryManager>` | Long-term memory provider registry |
| `tic_manager` | `Arc<TicManager>` | Background MCP event processor |
| `event_bus` | `Arc<ChatEventBus>` | In-process broadcast bus for completed chat turns |

---

---

## Workspace Crates

The binary depends on several independent library crates in `crates/`. Each crate has no dependency on the main `skald` crate and can be published or reused standalone.

| Crate | Path | Purpose |
| --- | --- | --- |
| `core-api` | `crates/core-api/` | Shared types and traits: `ServerEvent`, `GlobalEvent`, `InterfaceTool`, `SendMessageOptions`, `ChatHubApi` trait |
| `llm-client` | `crates/llm-client/` | LLM client abstraction (OpenAI-compat, Anthropic, Ollama) |
| `mcp-client` | `crates/mcp-client/` | MCP client (JSON-RPC over stdio/SSE) |
| `honcho-client` | `crates/honcho-client/` | Honcho long-term memory HTTP client |

### `core-api` — plugin extraction boundary

`core-api` is the designated contract crate for plugin independence. A plugin that depends only on `core-api` (instead of the full main crate) can be extracted into its own workspace member without circular dependencies.

**Current state of `ChatHubApi`:** `ChatHub` in the main crate implements `core_api::chat_hub::ChatHubApi`. Plugins that need to send messages, subscribe to events, or resolve approvals should program against `Arc<dyn ChatHubApi>` rather than `Arc<ChatHub>` directly.

See [workspace-crates.md](workspace-crates.md) for the full extraction roadmap.

---

## When to Update This File

- A new top-level struct is added to `AppState`
- The startup sequence in `main.rs` changes order or gains a new step
- The request lifecycle changes (new event type, new loop behavior)
- A new circular dependency and its resolution is introduced
- A new workspace crate is added
