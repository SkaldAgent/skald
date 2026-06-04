# Tools

## Tool Trait

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;            // JSON Schema object
    fn execute(&self, args: Value) -> Result<String>;
    fn category(&self) -> ToolCategory;              // access-control grouping
    fn sub_agents_only(&self) -> bool { false }      // default impl
    fn openai_definition(&self) -> Value { ... }     // default impl, rarely overridden
}
```

**`execute` is synchronous.** For async I/O inside a tool (e.g. DB queries), use `tokio::task::block_in_place(|| Handle::current().block_on(...))`. See `src/tools/cron_jobs.rs` for the pattern.

**`sub_agents_only`**: if a tool returns `true`, it is excluded from the root agent's tool list and only added to sub-agent configs (depth ≥ 1) in `dispatch_call_agent`. Default is `false` — all existing tools are visible to all agents.

---

## ToolCategory

Every tool declares a `ToolCategory`, used for access-control filtering and audit:

| Variant | Used by |
| --- | --- |
| `Filesystem` | File read/write tools (`read_file`, `write_file`, `edit_file`, …) |
| `Shell` | `execute_cmd`, `restart` |
| `Subagent` | `call_agent` (synthetic — not in registry) |
| `Introspection` | `list_agents`, `list_mcp`, `list_plugins`, `list_cron_jobs`, `image_generate_providers_list` |
| `Config` | `register_mcp`, `toggle_mcp`, `add_cron_job`, `delete_cron_job`, `toggle_cron_job`, `toggle_plugin`, `configure_plugin`, `image_generate`, `set_secret`, `list_secrets` |

---

## ToolRegistry

`HashMap<String, Arc<dyn Tool>>` with four public methods:

| Method | Purpose |
| --- | --- |
| `register(tool)` | Insert tool keyed by `tool.name()` |
| `openai_definitions()` | Returns definitions for root-agent tools (excludes `sub_agents_only`) |
| `openai_definitions_sub_agents_only()` | Returns definitions for tools where `sub_agents_only() == true` |
| `list_all()` | Returns `(name, description)` for all registered tools (sorted) |
| `dispatch(name, args)` | Executes tool by name; errors on unknown name |
| `describe_call(name, args, length)` | Returns a human-readable label for any tool call (including non-registry tools). Falls back to `name` for unknown tools. |

---

## Registration Pattern

All tools are registered in `src/main.rs` before `ChatSessionManager` is built.

**Not in ToolRegistry — synthetic tools intercepted in `run_agent_turn`:**

- `call_agent` — delegates to a sub-agent
- `update_scratchpad` — writes to `session_scratchpad` table; available to all agents
- `ask_user_clarification` — pauses and asks the user a question; routing depends on session type:
  - **Interactive sessions** (web, Telegram): available to sub-agents only (`depth ≥ 1`); emits `ServerEvent::AgentQuestion`, waits inline
  - **Background sessions** (cron, tic): available at root level (`!is_interactive`); registers with `ClarificationManager`, visible in Agent Inbox; agent suspends until answered
- `show_mcp_tools` — activates MCP servers for the session (lazy loading); injected as an `InterfaceTool` in `build_agent_config` with per-session state; not available to sub-agents
- `notify` — queues a notification briefing to the home conversation via `ChatHub`; **injected as an `InterfaceTool` by the caller** (`TicManager` for TIC, `CronTaskManager` for the worker agent); not in ToolRegistry so ordinary agents cannot call it

**Also not in ToolRegistry:**

- MCP tools — injected dynamically per-request via `McpManager::tools()`

---

## Per-Agent Tool Filtering (`allow_tools`)

An agent's `meta.json` can declare `allow_tools: ["tool_a", "tool_b"]`. When present, only those system tools are injected into the LLM's tool list for that agent's turn. Absent or `null` means all tools are available.

**MCP tools are never filtered** — they pass through regardless of `allow_tools`. The Approval gate governs MCP tool execution.

Filtering happens in `src/session/handler/config.rs` after assembling `base_tool_defs` (registry + synthetic tools), before extending with MCP tools.

---

## Built-in Tool Catalogue

| Tool name | Module | Category | Approval | Sub-agents only |
| --- | --- | --- | --- | --- |
| `list_files` | `tools::fs` | Filesystem | No | No |
| `read_file` | `tools::fs` | Filesystem | No | No |
| `write_file` | `tools::fs` | Filesystem | Yes (non-memory/) | No |
| `edit_file` | `tools::fs` | Filesystem | Yes (non-memory/) | No |
| `insert_at_line` | `tools::fs` | Filesystem | Yes (non-memory/) | No |
| `replace_lines` | `tools::fs` | Filesystem | Yes (non-memory/) | No |
| `search_file` | `tools::fs` | Filesystem | No | No |
| `grep_files` | `tools::fs` | Filesystem | No | No |
| `get_ast_outline` | `tools::ast_outline` | Filesystem | No | No |
| `execute_cmd` | `tools::exec` | Shell | **Always** | No |
| `restart` | `tools::restart` | Shell | **Always** | No |
| `list_agents` | `tools::list_agents` | Introspection | No | No |
| `list_mcp` | `tools::list_mcp` | Introspection | No | No |
| `list_plugins` | `tools::list_plugins` | Introspection | No | No |
| `list_cron_jobs` | `tools::cron_jobs` | Introspection | No | No |
| `register_mcp` | `tools::register_mcp` | Config | No | No |
| `toggle_mcp` | `tools::toggle_mcp` | Config | No | No |
| `add_cron_job` | `tools::cron_jobs` | Config | No | No |
| `delete_cron_job` | `tools::cron_jobs` | Config | No | No |
| `toggle_cron_job` | `tools::cron_jobs` | Config | No | No |
| `toggle_plugin` | `tools::toggle_plugin` | Config | No | No |
| `configure_plugin` | `tools::configure_plugin` | Config | No | No |
| `set_secret` | `tools::set_secret` | Config | No | No |
| `list_secrets` | `tools::list_secrets` | Config | No | No |
| `image_generate_providers_list` | `tools::image_generate` | Introspection | No | No |
| `image_generate` | `tools::image_generate` | Config | No | No |
| `update_scratchpad` | synthetic | — | No | No |
| `ask_user_clarification` | synthetic | — | No | Interactive: sub-agents only; Background: root level |
| `show_mcp_tools` | synthetic (per-session) | Config | No | No |

---

## Tool Display Labels

Every `Tool` implementation can override `describe(&self, args: &Value, length: ToolDescriptionLength) -> String` to produce a compact human-readable label shown in the UI and on Telegram instead of the raw tool name.

| Length | Max chars | Example |
| --- | --- | --- |
| `Short` | 60 | `execute_cmd \`git\`` |
| `Full` | 120 | `execute_cmd \`git commit -m "feat: ..."\`` |

Constants `MAX_LABEL_SHORT` and `MAX_LABEL_FULL` are defined in `src/tools/mod.rs`. `truncate_label(s, max)` truncates at char boundary appending `…`.

The default implementation returns `self.name()`, so all tools work without implementing `describe`. Built-in tools (fs, exec) have explicit implementations; MCP and plugin tools fall back to the tool name.

`ToolRegistry::describe_call(name, args, length)` is the single call-site used by `llm_loop.rs`, `resume.rs`, and the `/api/{source}/messages` history endpoint. It also handles synthetic tools (`call_agent`) that are not in the registry.

Labels are emitted in `ServerEvent::ToolStart` as `label_short` and `label_full` and included in history responses so the frontend always has them.

---

## FS Path Resolution

`tools::fs::resolve(path)`:

- If path starts with `/` → used as absolute path
- Otherwise → resolved relative to CWD (project root when running via `run.sh`)

Paths starting with `memory/` bypass the approval gate for write tools.

---

## Adding a Tool

1. Create a struct in `src/tools/` (new file or existing module).
2. `impl Tool` for the struct — include `fn category()`.
3. Register in `src/main.rs`: `tool_registry.register(MyTool::new(...))`.
4. If the tool needs `ChatHub` or should only be visible to specific callers (background agents), do **not** add it to `ToolRegistry` — implement it as an `InterfaceTool` and inject it at the call site (see `tools::notify::make_tool`).
5. If the tool needs user approval before executing, add it to `needs_approval()` in `src/session/handler/approval.rs`.
6. Update this doc (catalogue table).

---

## When to Update This File

- A tool is added, removed, or renamed
- The approval rules for a tool change
- The `Tool` trait gains or loses a method
- `ToolCategory` gains a new variant
- The `allow_tools` filtering logic changes
