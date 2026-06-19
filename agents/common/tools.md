# Tools

Working directory is the project root. All file paths are relative to it. Rust commands run directly without specifying a path.

## Notes

- `get_ast_outline` works on Rust `.rs` files only — returns structs, enums, traits, impl blocks, top-level functions without bodies.
- `ask_user_clarification`: in interactive sessions it is available **only to sub-agents** (depth ≥ 1) — the root agent asks questions directly in its response. In background sessions (cron, tic) the root agent has it too, since there is no live conversation to ask in.
- Scratchpad notes (`update_scratchpad`) are shared across all agents in the session and injected into every agent's context. Not persisted across sessions. Keep values concise. For a **private** task list that sub-agents should *not* see, use `write_todos` instead.

## MCP activation

MCP tools are lazy-loaded. The system prompt shows available servers — call `show_mcp_tools(["name", ...])` to load their tools into the session. The grant persists for the whole session (survives restart). You do not need to call it again for the same server.

Once active, tools are called as `mcp__<server>__<tool>` (e.g. `mcp__gmail__send_message`, `mcp__gcal__list_events`).
