# Agents

## Directory Layout

```text
agents/
  <id>/
    meta.json   ← required: metadata, LLM preferences
    AGENT.md    ← required: system prompt
  common/       ← shared include files; skipped by discover()
```

`agents::discover()` scans every subdirectory except `common/`. A directory without both files is skipped with a `WARN` log.

---

## meta.json Schema

| Field | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `name` | string | yes | — | Display name |
| `description` | string | yes | — | Used in `list_agents` output and `AGENTS_LIST` directive |
| `inject_memory` | `string[]` | no | `[]` | Paths to files injected into the system prompt as `<memory_file>` blocks |
| `client` | string \| null | no | null | Pin to a specific named LLM model (must exist in DB) |
| `scope` | string \| null | no | null | Task domain for AUTO client selection |
| `strength` | LlmStrength \| null | no | null | Minimum LLM capability for AUTO selection |
| `allow_tools` | `string[]` \| null | no | null | Whitelist of system tool names visible to this agent. `null` = all tools. MCP tools always pass through regardless. |
| `is_system_agent` | bool | no | `false` | When `true`, excluded from `list_agents` output and `AGENTS_LIST` injection. The main agent cannot see or call it. Use for background/system agents (e.g. TIC). |

### `allow_tools` filtering

When `allow_tools` is set, only those system tool names are injected into the LLM's tool list. The filter runs in `src/core/session/handler/config.rs` before the LLM call. MCP tools are excluded from filtering — the Approval gate governs them.

```json
{
  "name": "TIC",
  "allow_tools": ["read_file", "list_files", "list_agents", "list_mcp"]
}
```

---

## AGENT.md Directives

| Directive | Behavior |
| --- | --- |
| `<!-- INCLUDE: path/to/file.md -->` | Replaced with the content of `agents/path/to/file.md` at load time. Supports recursive includes. |
| `<!-- AGENTS_LIST -->` | Replaced with a bullet list of agents where `id != "main"` and `is_system_agent != true`: `- **id** — description` |
| `<!-- KEY -->` (any uppercase name) | Runtime substitution sentinel. Replaced at request time via `SendMessageOptions::system_substitutions`. The agent's system prompt contains `__KEY__` which is swapped for the provided value before the LLM call. |

---

## Available Agents

| id | name | scope | strength | system | description |
| --- | --- | --- | --- | --- | --- |
| `main` | Main Assistant | — | — | ✓ | General-purpose; persists notes in `data/memory/index.md` |
| `architect` | Architect | `reasoning` | `high` | | Plans code changes and delegates to engineer |
| `engineer` | Engineer | `coding` | `high` | | Writes and modifies source files across any file type |
| `researcher` | Researcher | `general` | `average` | | Multi-step web research; returns a structured summary and saves findings to the scratchpad |
| `worker` | Worker | — | — | ✓ | Autonomous background task executor for scheduled jobs. Uses sub-agents for complex work. Ephemeral per run. Not conversational — produces a final response captured as completion notification. |
| `tic` | TIC | — | — | ✓ | Background event processor; calls `notify` when something is worth surfacing. Ephemeral. `notify` is injected as an `InterfaceTool` by `TicManager` — not in `allow_tools`. |

---

## call_agent Mechanics

`call_agent` is **not** in `ToolRegistry`. It is intercepted in `run_agent_turn` before any registry lookup, then handled by `dispatch_call_agent`:

1. Validate `agent_id` and `prompt` args.
2. Reject self-calls and calls to `main`.
3. Reject calls to system agents (`meta.json` → `is_system_agent: true`) — they are invisible to call_agent.
4. Load target agent's `meta.json`.
4. Check depth: `parent_frame.depth + 1 <= MAX_AGENT_DEPTH`.
5. Resolve target client (see below).
6. Create child `chat_sessions_stack` row (`depth = parent + 1`, `parent_tool_call_id` set).
7. Load any existing `stack_mcp_grants` for the child stack (restart recovery) → populate `active_mcp_grants`.
8. Build child `AgentRunConfig` via `for_sub_agent()`, then:
   - Replace `active_mcp_grants` with the pre-populated arc from step 7.
   - Append `sub_agents_only` tools and `ask_user_clarification`.
   - Inject `show_mcp_tools` (stack-scoped, `stack_id = Some(child.id)`) as interface tool.
9. Append prompt as `role = agent` message in child stack.
10. Emit `AgentStart` event.
11. **Spawn** an independent `tokio::spawn` task running `run_child_frame` (see below).
12. Return `Err(WaitingChildSentinel(child_stack_id))` — the parent's LLM loop detects this and exits with `TurnOutcome::WaitingChild`, releasing the `processing` mutex.

The parent's `call_agent` tool call remains in status `running` in the DB until the child task completes it.

### run_child_frame (dispatcher.rs)

The spawned task acquires the `processing` mutex independently (parent has already released it) and:

1. Calls `resume_pending_tools` + `run_agent_turn` on the child stack.
2. Deletes `stack_mcp_grants` for the child stack.
3. On `Final`: emits `AgentDone`, marks parent's tool call `done`, emits `ToolDone`.
4. On `Cancelled`/`Exhausted`/`Err`: emits `AgentDone`, marks parent's tool call `failed`, emits `ToolError`.
5. Terminates the child stack frame.
6. Drops the processing lock.
7. Calls `resume_turn` so the parent LLM loop continues with the child's result in history.

If the child itself spawns a grandchild (`TurnOutcome::WaitingChild`), `run_child_frame` returns immediately — the grandchild task handles cascading back up.

### Mutex invariant

Parent and child never hold `processing` at the same time. Sequence:

- Parent acquires → runs → exits with `WaitingChild` → **releases**.
- Child task acquires → runs → **releases** → calls `resume_turn` (re-acquires).

---

## Client Resolution Order

For a `call_agent` call to agent `X`:

1. `args.client` (explicit override in the tool call)
2. `X/meta.json` → `client` field (pinned model name)
3. AUTO selection using `X/meta.json` → `scope` + `strength`

> **Important:** the parent agent's resolved client is **not** inherited by sub-agents.
> Passing a concrete model name to `resolve()` bypasses scope/strength checks entirely,
> so sub-agents always auto-select unless an explicit override is provided via (1) or (2).
> This ensures `strength: high` in `meta.json` is always respected regardless of which
> model the caller is using.

---

## Depth Limit

**`MAX_AGENT_DEPTH = 5`** (hardcoded in `src/core/session/handler/mod.rs`).

Depth 0 = root `main` session. Each `call_agent` increments depth by 1. Attempting to exceed the limit returns an error to the LLM without calling the sub-agent.

An agent cannot call itself or the `main` agent.

---

## Adding an Agent

1. Create `agents/<id>/meta.json` with at minimum `name` and `description`.
2. Create `agents/<id>/AGENT.md` with the system prompt.
3. Optionally add `allow_tools` to limit visible system tools.
4. **No restart required** — agents are discovered on every request.

---

## When to Update This File

- An agent is added, removed, or its meta fields change
- `MAX_AGENT_DEPTH` constant changes
- `call_agent` validation logic changes (new restrictions or resolution order)
- `meta.json` schema gains a new field
