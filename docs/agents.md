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
| `description` | string | yes | — | Used in `list_items` (type=agents) output and `AGENTS_LIST` directive |
| `inject_memory` | `string[]` | no | `[]` | Paths to files injected into the system prompt as `<memory_file>` blocks. Relative paths resolve against Skald's process cwd. The `$WD` placeholder expands to the session's effective working directory (RunContext WD, or process cwd when unset) — e.g. `"$WD/SKALD.md"` loads a project-local file. The path **shown** in the block is relative to the working directory when the file is under it, absolute otherwise — so it always resolves back to the same file under the loop's working-directory injection. Missing files inject a `(file not created yet)` placeholder. |
| `client` | string \| null | no | null | Pin to a specific named LLM model (must exist in DB) |
| `scope` | string \| null | no | null | Task domain for AUTO client selection |
| `strength` | LlmStrength \| null | no | null | Minimum LLM capability for AUTO selection |
| `is_system_agent` | bool | no | `false` | When `true`, excluded from `list_items` (type=agents) output and `AGENTS_LIST` injection. The main agent cannot see or call it. Use for background/system agents (e.g. TIC). |
| `inject_skills` | bool | no | `true` | When `true` (the default, **including when the key is absent**), the skills registry (`skills/index.md`) is injected into the system prompt as a `<skills_index>` block so the agent can discover installed skills. Path resolution follows the `inject_memory` rule (relative under the working directory, absolute otherwise). Skipped silently if no skills are installed. Set `false` for background agents that don't need them. |

### Tool restriction

Tool restriction is **not** declared in the agent file (the per-agent `allow_tools` whitelist and the `run_context` default were both removed). Tool visibility and execution-time approval are governed uniformly by **permission groups** bound to **run contexts** (see [approval/index.md](approval/index.md)).

A run context is assigned to a **session** at runtime, never in `meta.json`:

- explicitly via the UI / API (`set_session_run_context`),
- via a dedicated config property for system sessions (e.g. TIC's `tic.run_context`),
- per cron job (`run_context_id`).

When a session has no run context it uses the built-in **"default"** group. The visibility filter (hide tools whose effective action for the session's group is `Deny`) runs in `src/core/session/handler/config.rs` (depth 0) and `agent_dispatch.rs` (sub-agents). MCP tools are excluded from this filter — the Approval gate governs them.

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
| `researcher` | Researcher | `general` | `average` | | Multi-step web research; writes structured Markdown reports to `data/research/` and saves the path to scratchpad |
| `worker` | Worker | — | — | ✓ | Autonomous background task executor for scheduled jobs. Uses sub-agents for complex work. Ephemeral per run. Not conversational — produces a final response captured as completion notification. |
| `tic` | TIC | — | — | ✓ | Background event processor; calls `notify` when something is worth surfacing. Ephemeral. `notify` is injected as an `InterfaceTool` by `TicManager`. Tool access is restricted via the run context set from the `tic.run_context` property. |
| `explorer` | Explorer | `analysis` | `average` | | Studies code, investigates bugs, analyses architecture, produces structured Markdown reports in `data/explorer/`. No implementation. |
| `blueprint` | Blueprint | `reasoning` | `very_high` | | Transforms project ideas into comprehensive spec documents in `data/`. Never writes code. Saves output path to scratchpad. |
| `tech-lead` | Tech Lead | `reasoning` | `very_high` | | Reads project documentation, breaks scope into implementation tasks, sequences them by dependency, and orchestrates `architect`/`engineer` sub-agents to deliver end-to-end. Tracks its plan with `write_todos` (private, not `update_scratchpad`) and owns the single authoritative build+test run. |
| `project-coordinator` | Project Coordinator | `reasoning` | `average` | ✓ | Conversational coordinator for a single project's interactive chat (source `project-{id}`) — **any kind** of project (software, travel, study, writing, personal goals…), adapting to the injected description. Receives the project context via its session `RunContext` (working dir, description, fs-write grants), does everyday planning/writing itself, and delegates specialized work (research, or code via tech-lead/architect/engineer) via `execute_task`. Maintains the project's `SKALD.md` diary. System agent — not callable as a sub-agent or ticket agent. |


### Orchestration model (tech-lead + engineer)

Two conventions keep multi-agent builds efficient and avoid context pollution:

- **Private plan, not shared state.** `tech-lead` records its task plan and progress with `write_todos` — a stateless, per-stack list that lives only in its own tool-result history. It must **not** use `update_scratchpad` for the plan: the scratchpad is a shared blackboard injected into every sub-agent, so a plan written there would pollute each `engineer`'s context. The scratchpad stays reserved for genuine cross-agent communication (e.g. a discovered path or type).
- **Verify once, at the top.** `engineer` runs only a fast compile-check (e.g. `cargo check`) after writing — never the test suite. `tech-lead` owns the single full build + test run in its integration phase, against the merged result. This replaces the old pattern of N engineers each running a full build+test.

---

## Sub-agent Mechanics

A synchronous sub-agent call (`execute_task` mode=sync / `run_subtask`) is **not** in `ToolRegistry`. It is intercepted in `run_agent_turn` before any registry lookup, then handled by `dispatch_sub_agent` (`src/core/session/handler/agent_dispatch.rs`):

1. Validate `agent_id` and `prompt` args.
2. Reject self-calls and calls to `main`.
3. Reject calls to system agents (`meta.json` → `is_system_agent: true`) — they are invisible as sub-agents.
4. Load target agent's `meta.json`.
5. Check depth: `parent_frame.depth + 1 <= MAX_AGENT_DEPTH`.
6. Resolve target client (see below).
7. Create child `chat_sessions_stack` row (`depth = parent + 1`, `parent_tool_call_id` set).
8. Load any existing `stack_mcp_grants` for the child stack (restart recovery) → populate `active_mcp_grants`.
9. Build child `AgentRunConfig` via `for_sub_agent()`, then:
   - Replace `active_mcp_grants` with the pre-populated arc from step 8.
   - Append `sub_agents_only` tools and `ask_user_clarification`.
   - Append `run_subtask` (so the child can dispatch its own sub-agents, e.g. `tech-lead` → `architect`/`engineer`) — **only** when `depth + 1 < MAX_AGENT_DEPTH`, since at the limit the call would be rejected anyway. `for_sub_agent()` clears all interface tools (including the root's `execute_task`/`run_subtask` interface tools), so without this re-injection a sub-agent has **no** tool definition to delegate further; the call itself is then intercepted in `run_agent_turn` and routed back through `dispatch_sub_agent`.
   - Inject `show_mcp_tools` (stack-scoped, `stack_id = Some(child.id)`) as interface tool.
10. Append prompt as `role = agent` message in child stack.
11. Emit `AgentStart` event.
12. **Run the child inline** — `resume_pending_tools` + `run_agent_turn` on the child stack, awaited recursively **in the same task**, holding the **same** `processing` lock and the **same** `CancellationToken` clone.
13. Delete `stack_mcp_grants` for the child stack; emit `AgentDone`; terminate the child stack frame.
14. Map the child `TurnOutcome` to the return value: `Final{content}` → `Ok(content)`; `Cancelled` → `Ok("…cancelled")` (the shared token also stops the parent at its next check); `Exhausted` → `Ok("…exceeded rounds")`; `Err` → `Err`.

The returned string becomes the parent's tool-call result via the normal `Ok(result)` branch in `run_agent_turn` (which calls `chat_llm_tools::complete` and emits `ToolDone`) — so completion logic lives in exactly one place. There is **no** task spawn, no `WaitingChild` signal, and no resume cascade for the sync path.

### Mutex / token invariant

One user message = one logical critical section: the `processing` lock is acquired once in `handle_message` and held for the whole parent+child recursion. Parent and child share one `CancellationToken` clone, so a `/stop` that cancels a running child stops the parent by construction (its next round/tool check observes `is_cancelled()`).

`resume_turn` and its cascade are retained only for app-restart recovery of an active child stack, async task result injection (`inject_async_result`), and the WS resume message — not for normal sync dispatch.

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
3. Optionally restrict tools by assigning the agent's sessions a run context (permission group) at runtime — see [approval/index.md](approval/index.md).
4. **No restart required** — agents are discovered on every request.

---

## When to Update This File

- An agent is added, removed, or its meta fields change
- `MAX_AGENT_DEPTH` constant changes
- `call_agent` validation logic changes (new restrictions or resolution order)
- `meta.json` schema gains a new field
