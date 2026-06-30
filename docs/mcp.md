# MCP (Model Context Protocol)

## Workspace Location

The MCP protocol layer lives in the standalone crate `crates/mcp-client`:
- `McpServer` — stdio subprocess client
- `McpHttpServer` — streamable HTTP client
- `McpServerClient` trait, `McpTool`, `McpServerConfig`, `McpTransport`

`McpManager` (`src/core/mcp/mod.rs`) remains in the main crate because it owns the `SqlitePool` and calls `crate::db::mcp_events` / `crate::db::mcp_servers`.

---

## What MCP Is Here

MCP allows external processes or HTTP services to expose tools to the LLM. The app connects to MCP servers at startup (or on demand via `register_mcp`), discovers their tools, and makes them available alongside built-in tools.

---

## McpManager Internals

```rust
McpManager {
  pool:    Arc<SqlitePool>
  servers: RwLock<HashMap<String, Arc<dyn McpServerClient>>>  // running servers
  errors:  RwLock<HashMap<String, String>>                    // startup failures
}
```

Initialization runs in a background `tokio::spawn` task. The manager is available immediately; servers connect asynchronously. A server failing to start is recorded in `errors` and does not block the app.

---

## Transports

| Transport | When to use | Required fields |
| --- | --- | --- |
| `stdio` | Local process (spawn subprocess) | `command`, optionally `args`, `env` |
| `http` | Remote HTTP server (streamable MCP) | `url`, optionally `api_key` |
| `sse` | Alias for `http` (backward compat) | same as `http` |

`${VAR}` interpolation is supported in `env` values and `api_key`.

### stdio process lifecycle

stdio subprocesses are spawned with `kill_on_drop(true)` and, on Unix, in their
own process group (`process_group(0)`). The new process group detaches them from
the terminal's foreground group, so a terminal Ctrl+C (SIGINT to the whole group)
does not reach them directly — otherwise Python-based servers would catch it and
dump a `KeyboardInterrupt` traceback. They are instead reaped via `kill_on_drop`:
when the app shuts down and the per-server reader task is dropped, the child gets
a silent SIGKILL.

The child's **stderr is captured** (`Stdio::piped()`, not inherited) and drained
into `tracing` at `debug` level under the `mcp_client` target, prefixed with the
server name. This keeps startup banners, deprecation warnings and INFO logs from
servers like FastMCP off the console at the default log level, while still making
them available for diagnostics via `RUST_LOG=mcp_client=debug`.

---

## Protocol version, header & pagination

**Protocol version** is a single shared constant — `PROTOCOL_VERSION` in
`crates/mcp-client/src/lib.rs` (currently **`2025-11-25`**, the revision Skald
targets). Both transports advertise it in their `initialize` request, so they can
never drift apart. **Capabilities are per-transport**, not shared: stdio declares
`{ "elicitation": {} }` (form mode — see Elicitation below); HTTP declares `{}`
because it does not service the `ElicitationHandler` (stdio-only) and must not
claim a capability it can't honour.

**Version negotiation is tolerant.** Skald reads `protocolVersion` from the
`initialize` response and, if the server negotiates a different (older) version,
logs a `warn!` and proceeds rather than disconnecting.

**`MCP-Protocol-Version` header (HTTP only).** Per the Streamable HTTP spec, every
*post-initialize* request must carry `MCP-Protocol-Version: <negotiated>`. The HTTP
transport captures the negotiated version into `protocol_version: Mutex<Option<String>>`
(mirroring how `session_id` is captured) and `request_headers()` injects it on every
`request`/`notify`. It is `None` only during the `initialize` call itself, so the
header is naturally omitted there (the spec scopes it to post-initialize requests).

**`tools/list` pagination.** Both transports follow the cursor: `tools/list` is
requested with `{ "cursor": <nextCursor> }` until the response omits `nextCursor`,
accumulating every page (previously only the first page was read, silently
truncating large servers). A `MAX_TOOL_PAGES` (50) cap guards against a server that
never clears the cursor. The per-tool field mapping lives in one place,
`McpTool::from_json`, shared by both transports.

---

## Structured tool results

MCP tools with an `outputSchema` return `structuredContent` (a JSON object) in
addition to (or instead of) text. Skald preserves the type end-to-end instead of
flattening everything to a string:

- `McpCallResult` (`crates/mcp-client/src/lib.rs`) — the transport-level result:
  `Text(String)` or `Json(Value)`. `extract_call_result` **prefers
  `structuredContent`** when present (canonical per spec) and falls back to the
  joined `text` items — which also fixes the silent empty-result case for servers
  that return only `structuredContent` without the recommended text mirror.
  > Tradeoff: when a server returns *both* a text mirror and `structuredContent`,
  > the LLM sees the (compact) JSON, not the text. With a single `result` column +
  > a type tag this is the correct one-representation choice (JSON is lossless and
  > LLM-readable; the mirror is usually just `JSON.stringify` of the same object).
- `McpManager::call` maps `McpCallResult` → `ToolResult` (`crates/core-api/src/tool.rs`),
  the host-side equivalent (`Text`/`Json`). `ToolResult::to_wire()` is the string
  persisted in `chat_llm_tools.result` and replayed to the LLM (Json → compact JSON
  string); `ToolResult::kind()` is the `"string"`/`"json"` tag.
- The tag is persisted in `chat_llm_tools.result_type` (schema **v19**, `DEFAULT
  'string'`, `CHECK IN ('string','json')`) and sent to the frontend both live
  (`ServerEvent::ToolDone.result_type`) and on history replay / approval-resolve
  (`/api/sessions` items + `ResolveToolResponse`).
- Frontend: `copilot-render.js` renders a `result_type === 'json'` result as
  pretty-printed JSON (`.copilot-tool-pre--json`); everything else stays plain text.

`McpTool` also captures `title`, `output_schema`, and `annotations` (2025-06-18+).
These are stored but **not yet** validated/surfaced (output-schema validation,
`readOnlyHint`/`destructiveHint` UI hints are future work).

---

## Tool Naming Convention

MCP tools are exposed to the LLM as **`mcp__<server_name>__<tool_name>`**.

Examples:

- Server `tavily`, tool `search` → `mcp__tavily__search`
- Server `fetch`, tool `get` → `mcp__fetch__get`

`parse_mcp_tool_name(name)` in `src/core/mcp/mod.rs` splits on `__` to extract server and tool names. This is how `run_agent_turn` routes MCP calls.

---

## Registering a Server

All MCP servers are stored in the **`mcp_servers` table** in SQLite. There is no static config file.

**Live registration** via `register_mcp` tool:

- LLM calls `register_mcp` with name, transport, connection details, and optionally `description` and `friendly_name`
- `McpManager::register()` does DB upsert + live `start_one()` connect
- Server is immediately available without a restart

**Tool parameters:**

| Parameter | Required | Type | Description |
| --- | --- | --- | --- |
| `name` | yes | string | Unique name for this MCP server (used to reference it in tool calls) |
| `transport` | yes | string | `stdio`, `http`, or `sse` |
| `command` | stdio only | string | Executable to spawn |
| `args` | stdio only | string[] | Command-line arguments |
| `env` | stdio only | object | Extra environment variables |
| `url` | http/sse only | string | Base URL of the remote server |
| `api_key` | http/sse only | string | API key (sent as `Authorization: Bearer <key>`) |
| `description` | no | string | Short description of what the server provides (shown in `list_items` type=mcp) |
| `friendly_name` | no | string | Human-readable display name for UI (e.g. "Google Calendar") |

**Startup timeout**: **`SERVER_START_TIMEOUT_SECS = 120`**. Servers that don't respond within 120 s are recorded as errors.

---

## Enabling / Disabling Servers

Use the built-in tool **`toggle_item`** (kind=mcp) to enable or disable an MCP server by name:

```text
toggle_item(kind="mcp", id="gcal", enabled=false)  # disable
toggle_item(kind="mcp", id="gcal", enabled=true)   # enable
```

**Important:** Toggling updates the `enabled` flag in the database, but **a restart is required** for the change to take effect on running servers. Disabled servers won't connect on next restart.

Use `list_items` (type=mcp) to see current server names and statuses.

---

## Example: Google Calendar MCP Server

A custom Python MCP server (`scripts/gcal_mcp_server.py`) provides full read/write access to Google Calendar:

| Tool | Description |
| --- | --- |
| `list_calendars` | Lists all calendars accessible to the authenticated user |
| `list_events` | Lists events with filters: `calendar_id`, `start_time`, `end_time`, `max_results`, `full_text`, `time_zone` |
| `get_event` | Returns a single event by `event_id` |
| `create_event` | Creates a new event (`summary`, `start`, `end`, optional description/location/attendees/recurrence) |
| `update_event` | Updates an existing event — only fields provided are changed |
| `delete_event` | Permanently deletes an event by `event_id` |
| `respond_to_event` | Sets RSVP status (`accepted`, `declined`, `tentative`, `needsAction`) |

**Credentials:** Stored in `./secrets/google_creds.json`. Run `python3 scripts/gcal_oauth_setup.py` to authenticate (requires `https://www.googleapis.com/auth/calendar` scope). Token refresh is handled automatically.

**Register:**

```text
register_mcp(name="gcal", transport="stdio", command="python3", args=["scripts/gcal_mcp_server.py"])
```

**Disable when not needed:**

```text
toggle_item(kind="mcp", id="gcal", enabled=false)
restart
```

---

## Push Notifications from MCP Servers

MCP servers can send **unsolicited events** to the app by writing JSON-RPC notification messages (no `id` field) to stdout. The app persists them to SQLite and processes them in batches via the TIC background agent.

### Protocol

A notification is a JSON-RPC 2.0 message without `id`:

```json
{"jsonrpc": "2.0", "method": "event/new_email", "params": {"subject": "...", "from": "..."}}
```

### How it flows

```text
MCP server writes notification to stdout
  → McpServer reader loop detects msg with no "id"
  → sends (server_name, msg) over notification_tx channel
  → McpManager::notification_consumer persists to mcp_events table
  → TicManager (every `tic.interval_secs`, default 900 s) fetches pending events, runs TIC agent
  → TIC calls notify(briefing) if user action is needed
```

### Implementing notifications in an MCP server

**Node.js (WhatsApp)**:

```js
function notify(method, params) {
    process.stdout.write(JSON.stringify({jsonrpc:'2.0', method, params}) + '\n');
}
client.on('message', async (msg) => {
    if (msg.fromMe) return;
    notify('event/whatsapp_message', { from: msg.from, body: msg.body });
});
```

**Python (Gmail, GCal)** — use a lock to avoid interleaving with MCP responses:

```python
import threading
_stdout_lock = threading.Lock()

def _emit_notification(method, params):
    msg = json.dumps({"jsonrpc": "2.0", "method": method, "params": params})
    with _stdout_lock:
        sys.stdout.write(msg + "\n")
        sys.stdout.flush()
```

Start a daemon polling thread in `main()` before entering the MCP serve loop. The MCP serve loop must also acquire `_stdout_lock` before writing responses.

### Implemented notification sources

| Source | Method | Trigger | Poll interval |
| --- | --- | --- | --- |
| `whatsapp` | `event/whatsapp_message` | Inbound WhatsApp message | Real-time (event) |
| `gmail` | `event/new_email` | New email in INBOX | 60 s (History API) |
| `gcal` | `event/new_calendar_event` | New calendar event created | 300 s (Events API) |

---

## Elicitation — server-initiated input (spec 2025-06-18)

An MCP server can ask the user for input **during** a tool call — a server→client
request, distinct from the unsolicited notifications above. Primary use case: the
SSH MCP asking for a sudo password on demand (see `data/mcp_ssh.md`). The value
never reaches the LLM, is never logged, and is never persisted.

Skald advertises the capability on the **stdio** transport's `initialize`
(`"capabilities": { "elicitation": {} }`) and surfaces requests in the Agent Inbox.
The `protocolVersion` is the shared `PROTOCOL_VERSION` const (see *Protocol version,
header & pagination*); `{ "elicitation": {} }` is form mode, which is what Skald
supports (URL-mode elicitation, new in 2025-11-25, is not yet handled).

### Elicitation protocol

A server→client request has **both** `method` and `id`:

```json
{"jsonrpc":"2.0","id":"e1","method":"elicitation/create","params":{
  "message":"Enter sudo password",
  "requestedSchema":{"type":"object","properties":{
    "password":{"type":"string","format":"password"}}}}}
```

Skald replies on the same stdin: `{action: "accept"|"decline"|"cancel", content?: {…}}`.

### Elicitation flow

```text
MCP server writes elicitation/create (method + id) to stdout
  → McpServer reader loop routes it to handle_server_request (BEFORE the id/response
    branch, since it has both method and id) and spawns a task (the user may take minutes)
  → ElicitationHandler bridge (src/core/elicitation) → ElicitationManager::register
  → ServerEvent::ElicitationRequested → Agent Inbox card ("Secrets" section)
  → user enters a value (masked if sensitive) and confirms / rejects
  → POST /api/inbox/elicitations/{id}/resolve → ElicitationManager::resolve
  → bridge maps the outcome → reader loop writes the JSON-RPC reply to the server's stdin
```

While an elicitation is in flight, the underlying `tools/call` does **not** time out
(`pending_elicitations` counter re-arms the call timeout). On a 5-min user-response
deadline, channel drop, or `decline`/`cancel`, the server receives a non-accept reply.

The schema is **v1-scoped**: a single field (masked when `format: password`,
`writeOnly: true`, or the name contains `password`/`passphrase`/`secret`/`token`),
or an empty `properties` ⇒ a yes/no confirmation. Elicitation is **stdio-only**.

A dependency-free demo server lives at `scripts/elicitation_demo_mcp.py`.

---

## SSH MCP Server

`scripts/ssh_mcp_server.py` is a stdio MCP server (depends on `paramiko>=3.4`) that
operates on remote hosts. Its filesystem tools intentionally produce the **same output
format** as Skald's native fs tools (`read_file`, `list_files`, `grep_files`, `edit_file`,
`replace_lines`) so the LLM treats local and remote uniformly — the only difference is a
leading `alias` argument. Full design notes: `data/mcp_ssh.md`.

**13 tools** (bare names; Skald prefixes `mcp__ssh__`):

- Aliases: `list_aliases`, `add_alias`, `remove_alias`.
- Filesystem (SFTP, login user): `read_file`, `list_files`, `grep_files`, `edit_file`, `replace_lines`.
- Exec/transfer: `exec` (with `sudo`/`sudo_user`), `upload`, `download` (both recursive on directories).
- Diagnostics: `sysinfo`, `systemd`.

**Aliases** live in `secrets/ssh_aliases.json` (auto-managed by the tools, written
atomically at `0600`, gitignored). The file holds **no secrets** (no password, ever).
Host keys are checked against `~/.ssh/known_hosts`; unknown hosts are rejected unless the
alias was added with `accept_new_host_key=true`. Connections are pooled per alias with
lazy TTL eviction (`SSH_MCP_POOL_TTL`, default 300s).

**Login auth** (per-alias `auth`, default `key`):

- `key` → SSH key / ssh-agent. If the chosen private key is encrypted, its passphrase is
  requested **lazily** via elicitation (only when paramiko reports the key needs one).
  `SSH_MCP_KEY_PASSPHRASE` still works as a non-interactive override.
- `password` → login password requested on demand via **elicitation** (a masked field in
  the Agent Inbox); agent/key probing is skipped so paramiko goes straight to the password.

Elicited login secrets are cached only in the server's RAM (`SSH_MCP_LOGIN_PW_TTL`, default
300s), never sent to the LLM, never written to disk, and dropped on an authentication
failure so the next attempt re-prompts.

**sudo** (per-alias `sudo.method`, default `prompt`):

- `nopasswd` → `sudo -n`: non-interactive, fails fast if NOPASSWD isn't configured (no hung channel).
- `prompt` → `sudo -S`: the password is requested on demand via **elicitation** (see above),
  a masked single field; fed to sudo's stdin, cached only in the server's RAM
  (`SSH_MCP_SUDO_PW_TTL`, default 300s), never sent to the LLM, never written to disk.
- `none`: sudo disabled.

SFTP tools run as the login user (no root). For privileged writes the LLM is told to use
`exec(..., sudo=true)` with `tee`/`install`.

Register like any stdio server: `command: python3`, `args: ["scripts/ssh_mcp_server.py"]`.

---

## Lazy MCP Tool Loading

By default, injecting all MCP tool definitions into every LLM turn is expensive — 30+ tools can consume 10,000+ tokens per turn. Lazy loading solves this by only including tools for servers that have been explicitly activated.

### How It Works

1. At the start of each turn, `build_agent_config` reads `session_mcp_grants` for the current `session_id` and populates `active_mcp_grants` in memory.
2. **MCP tools are no longer part of `base_tool_defs`**. Instead, `AgentRunConfig::all_tool_defs()` re-queries `mcp.tools_for(active_mcp_grants)` on **every LLM round**. This means a `show_mcp_tools` call in round N makes those tools available from round N+1 within the same turn — no cross-turn delay.
3. The system prompt contains a `<!-- MCP_LIST -->` tag (in `AGENT.md`) which is replaced at request time with a dynamic two-section block:

   ```text
   ## MCP servers

   **Available** — call `show_mcp_tools(["name"])` to load tools:

   | Server     | Description                              |
   |------------|------------------------------------------|
   | `tavily`   | Web search and content extraction        |
   | `whatsapp` | Send and receive WhatsApp messages       |

   **Active** — tools callable as `mcp__<name>__<tool>`:
   - `gmail`
   ```

### `<!-- MCP_LIST -->` Tag

Add this tag anywhere in an `AGENT.md` to inject the dynamic MCP availability block at that position. Agents that do not include the tag receive no MCP list injection.

Currently used in: `agents/main/AGENT.md`, `agents/tic/AGENT.md`.

Resolution pipeline:

- `agents::resolve_includes()` — replaces `<!-- MCP_LIST -->` with the `__MCP_LIST__` sentinel.
- `ChatSessionHandler::build_openai_messages()` — replaces `__MCP_LIST__` with the rendered block (via `render_mcp_list()`).

### `show_mcp_tools` Tool

A synthetic interface tool (not in the global `ToolRegistry`):

```text
show_mcp_tools(mcp_names: ["server_name", ...])
```

- Takes an array of MCP server names.
- Updates the in-memory `active_mcp_grants` set immediately.
- **Root agents** (`stack_id = None`): persists grants to `session_mcp_grants` — survives across turns and restarts.
- **Sub-agents** (`stack_id = Some(id)`): persists grants to `stack_mcp_grants` — survives restarts, but **deleted when the stack frame terminates** (no session leak).
- Returns a confirmation string listing which servers were activated and their scope (`session` or `stack <id>`).

**Root agents**: injected in `build_agent_config` as an `InterfaceTool`.
**Sub-agents**: injected in `dispatch_sub_agent` — sub-agents always start with zero grants and activate what they need.

### Sub-Agent MCP Isolation

Sub-agents have a fully isolated MCP grant state:

| Aspect | Root agent | Sub-agent |
| --- | --- | --- |
| Initial grants | Loaded from `session_mcp_grants` DB | Empty (starts from zero) |
| `show_mcp_tools` persists to | `session_mcp_grants` | `stack_mcp_grants` |
| Grants survive restart? | Yes | Yes (re-loaded by `dispatch_sub_agent`) |
| Grants cleaned up? | No (session lifetime) | Yes (on frame termination) |
| Session contamination? | N/A | None |

Sub-agents that don't include `<!-- MCP_LIST -->` in their `AGENT.md` receive no MCP list injection in the system prompt. The tool definitions are still included dynamically in `all_tool_defs()` based on grants, so they can call tools without the descriptive list — useful for agents with a narrow, pre-known tool set.

### `tic` Agent

`tic` uses lazy loading like any other root agent — it calls `show_mcp_tools` for the servers it needs based on the pending events it receives. This avoids loading all MCP tool definitions on every tick when there may be nothing to process.

### Token Savings

| Situation | Approximate tokens |
| --- | --- |
| All MCP tools always loaded (old behaviour) | ~10,000–20,000 |
| Lazy mode, no grants yet | ~50–100 (compact list only) |
| Lazy mode, gmail + gcal granted | ~2,000–4,000 |

---

## When to Update This File

- A new transport type is added
- `PROTOCOL_VERSION` is bumped, the `MCP-Protocol-Version` header logic changes, or version-negotiation handling changes
- `tools/list` pagination (cursor loop, `MAX_TOOL_PAGES`) or `McpTool::from_json` changes
- The structured-result pipeline changes (`McpCallResult`/`ToolResult`, `result_type` column/event, `extract_call_result` preference, or the frontend JSON rendering)
- The tool naming convention changes
- `SERVER_START_TIMEOUT_SECS` changes
- `register_mcp` tool parameters change (schema, required fields, description, friendly_name)
- `list_items` (type=mcp) return format changes (McpServerInfo fields)
- A new notification source is implemented
- The elicitation flow changes (capability/protocol version, schema parsing, the resolve route, or the in-flight timeout behaviour)
- The SSH MCP server changes (tools, alias schema, sudo methods, or pooling/host-key behaviour in `scripts/ssh_mcp_server.py`)
- Lazy loading logic changes (`build_agent_config`, `dispatch_sub_agent`, `show_mcp_tools`, grant tables)
- `ClientMessage` loses or gains fields relevant to MCP
