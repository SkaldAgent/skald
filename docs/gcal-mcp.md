# Google Calendar MCP Server (gcal)

## Overview

A custom Python MCP server providing **read-only** access to Google Calendar via the Google Calendar API v3.

**Server name:** `gcal`  
**Transport:** `stdio` (spawns `python3 scripts/gcal_mcp_server.py`)  
**Location:** `scripts/gcal_mcp_server.py`

---

## Tools

| Tool | Parameters | Description |
|------|------------|-------------|
| `list_calendars` | *(none)* | Lists all calendars accessible to the authenticated user |
| `list_events` | `calendar_id` (string, default: "primary"), `start_time` (ISO 8601), `end_time` (ISO 8601), `max_results` (int, default: 100), `full_text` (string), `time_zone` (string, default: "Europe/London") | Lists calendar events with optional filters |
| `get_event` | `event_id` (required), `calendar_id` (string, default: "primary") | Returns details of a single event |

---

## Authentication

### Credentials File

OAuth 2.0 credentials are stored in:
- **Default path:** `./secrets/google_creds.json` (relative to project root)
- **Override:** Set `GOOGLE_CREDS_PATH` environment variable

### Git Safety

The `./secrets/` directory is listed in `.gitignore`, so credentials won't be committed.

### File Format

```json
{
  "token": "ya29.a0A...",
  "refresh_token": "1//03...",
  "token_uri": "https://oauth2.googleapis.com/token",
  "client_id": "760823396921-...",
  "client_secret": "GOCSPX-...",
  "scopes": [
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
    "https://www.googleapis.com/auth/calendar.events.freebusy",
    "https://www.googleapis.com/auth/calendar.events.readonly",
    "https://www.googleapis.com/auth/calendar.events"
  ],
  "expiry": "2026-05-19T12:13:26"
}
```

### Token Refresh

The server automatically refreshes the access token when it expires using the refresh token. No manual intervention required.

---

## Usage

### Register the Server

```bash
register_mcp(
  name="gcal",
  transport="stdio",
  command="python3",
  args=["scripts/gcal_mcp_server.py"]
)
```

### List Calendars

```bash
mcp__gcal__list_calendars()
```

### List Events (next 7 days)

```bash
mcp__gcal__list_events(
  calendar_id="primary",
  start_time="2026-05-19T00:00:00+01:00",
  end_time="2026-05-26T00:00:00+01:00",
  max_results=50,
  time_zone="Europe/London"
)
```

### Search Events

```bash
mcp__gcal__list_events(
  calendar_id="primary",
  full_text="meeting",
  start_time="2026-05-19T00:00:00+01:00",
  end_time="2026-06-19T00:00:00+01:00"
)
```

### Get Single Event

```bash
mcp__gcal__get_event(
  event_id="_6os3eohk71imcb9lcphjib9k6ksm2b9p71h32bb5cos32dhh60p36e35cc",
  calendar_id="primary"
)
```

---

## Enable / Disable

### Disable (when not needed)

```bash
toggle_item(kind="mcp", id="gcal", enabled=false)
restart  # required for change to take effect
```

### Re-enable

```bash
toggle_item(kind="mcp", id="gcal", enabled=true)
restart  # required for change to take effect
```

Disabled servers won't connect on next restart.

---

## Dependencies

The server requires these Python packages (already installed):
- `google-auth`
- `google-auth-oauthlib`
- `google-api-python-client`

### Install (if needed)

```bash
pip3 install google-auth google-auth-oauthlib google-api-python-client
```

---

## Error Handling

The server handles these error cases gracefully:

| Error | Response |
|-------|----------|
| Credentials file not found | `"Error: Credentials file not found at ~/.anthea/google_creds.json..."` |
| Invalid credentials JSON | `"Error: Failed to load credentials: ..."` |
| Token expired & refresh fails | `"Error: Failed to build Calendar service: ..."` |
| Unknown tool | `"Error: Unknown tool: <name>"` |
| Missing required parameter | `"Error: Missing required parameter '<param>'"` |

All errors are logged to stderr with `[gcal_mcp]` prefix for debugging.

---

## Protocol

Implements JSON-RPC 2.0 over stdio:
- **Requests:** Read JSON from stdin (one per line)
- **Responses:** Write JSON to stdout
- **Logs:** Write to stderr (prefixed with `[gcal_mcp]`)

Supported methods:
- `initialize` — MCP handshake
- `notifications/initialized` — MCP notification (no response)
- `tools/list` — Return available tools
- `tools/call` — Execute a tool

---

## When to Update This File

- New tools are added to the server
- Authentication mechanism changes
- Credential path or format changes
- New error cases are handled
- Protocol version changes
