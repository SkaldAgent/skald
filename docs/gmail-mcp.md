# Gmail MCP Server (gmail)

## Overview

A custom Python MCP server providing **read, modify, and send** access to Gmail via the Gmail API v1.

**Server name:** `gmail`  
**Transport:** `stdio` (spawns `python3 scripts/gmail_mcp_server.py`)  
**Location:** `scripts/gmail_mcp_server.py`

### Permissions (safe by design)

| Capability | Yes/No |
|------------|--------|
| Read messages & threads | ✅ |
| Search messages | ✅ |
| List labels | ✅ |
| Modify labels (mark read, star, archive) | ✅ |
| Send email | ✅ |
| Create labels | ✅ |
| Download attachments | ✅ |
| Trash message (reversible) | ❌ *removed from tools* |
| Untrash message | ❌ *removed from tools* |
| Permanently delete | ❌ *scope not granted* |

The server uses the `gmail.modify` scope which allows all operations **except permanent deletion**.

---

## Tools

| Tool | Parameters | Description |
|------|-----------|-------------|
| `list_messages` | `query` (string), `max_results` (int, default 20, max 50), `label_ids` (string[]) | List messages with optional Gmail search query and label filter |
| `get_message` | `message_id` (required), `include_body` (bool, default true) | Get full content of a message by ID |
| `get_thread` | `thread_id` (required) | Get all messages in a thread |
| `list_labels` | *(none)* | List all labels with message counts |
| `search_messages` | `query` (string), `max_results` (int), `label_ids` (string[]) | Search messages (alias for list_messages) |
| `modify_message` | `message_id` (required), `add_labels` (string[]), `remove_labels` (string[]) | Add/remove labels (mark read, archive, star) |
| `send_message` | `to` (required), `subject` (required), `body` (required), `cc` (string), `bcc` (string), `in_reply_to` (string), `thread_id` (string) | Send an email; supports in-thread replies |
| `get_profile` | *(none)* | Get profile info: email, total messages/threads |
| `create_label` | `name` (required), `label_list_visibility` (string, default "labelShow"), `message_list_visibility` (string, default "show") | Create a new Gmail label/folder |
| `download_attachments` | `message_id` (required), `folder` (string, default "uploads/gmail_attachments/") | Download all attachments from a message to a local folder |

### Gmail Search Syntax

The `query` parameter supports Gmail's native search syntax:

| Example | Meaning |
|---------|---------|
| `from:john@example.com` | Messages from a sender |
| `to:maria@example.com` | Messages to a recipient |
| `subject:meeting` | Messages with "meeting" in subject |
| `is:unread` | Unread messages |
| `is:starred` | Starred messages |
| `in:inbox` | Messages in Inbox |
| `in:sent` | Sent messages |
| `after:2024/01/01` | Messages after a date |
| `before:2024/06/01` | Messages before a date |
| `has:attachment` | Messages with attachments |
| `label:MY_LABEL` | Messages with a custom label |

Combine with spaces: `from:john is:unread after:2024/06/01`

---

## Authentication

### Credentials File

OAuth 2.0 credentials are stored in:
- **Default path:** `./secrets/gmail_creds.json` (relative to project root)
- **Override:** Set `GMAIL_CREDS_PATH` environment variable

### Git Safety

The `./secrets/` directory is listed in `.gitignore`, so credentials won't be committed.

### Setup (one-time)

```bash
python3 scripts/gmail_oauth_setup.py
```

This opens your browser, asks you to authorize the Gmail scopes, and saves the token to `secrets/gmail_creds.json`.

### Token Refresh

The server automatically refreshes the access token when it expires using the refresh token. No manual intervention required.

---

## Usage

### Register the Server

```bash
register_mcp(
  name="gmail",
  transport="stdio",
  command="python3",
  args=["scripts/gmail_mcp_server.py"]
)
```

### List Unread Messages

```bash
mcp__gmail__list_messages(query="is:unread", max_results=10)
```

### Get a Message Full Content

```bash
mcp__gmail__get_message(message_id="190abc123...", include_body=true)
```

### Mark as Read

```bash
mcp__gmail__modify_message(message_id="190abc123...", remove_labels=["UNREAD"])
```

### Archive (remove from inbox)

```bash
mcp__gmail__modify_message(message_id="190abc123...", remove_labels=["INBOX"])
```

### Send an Email

```bash
mcp__gmail__send_message(
  to="friend@example.com",
  subject="Hello!",
  body="How are you?"
)
```

### Reply in-thread

To reply to a message within its thread, pass the `in_reply_to` (message ID) and `thread_id` (thread ID) from the original message:

```bash
mcp__gmail__send_message(
  to="sender@example.com",
  subject="Re: Original subject",
  body="This is my reply.",
  in_reply_to="190abc123...",
  thread_id="190thread456..."
)
```

Both `in_reply_to` and `thread_id` are optional and independent — use either or both. `in_reply_to` adds RFC 2822 threading headers (`In-Reply-To` and `References`) visible to email clients, while `thread_id` tells Gmail to attach the message to a specific thread. When both are provided, the reply appears correctly threaded in all email clients.

---

## Enable / Disable

### Disable (when not needed)

```bash
toggle_item(kind="mcp", id="gmail", enabled=false)
restart  # required for change to take effect
```

### Re-enable

```bash
toggle_item(kind="mcp", id="gmail", enabled=true)
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
| Credentials file not found | `"Error: Credentials file not found at ..."` |
| Invalid credentials JSON | `"Error: Failed to load credentials: ..."` |
| Token expired & refresh fails | `"Error: Failed to refresh credentials: ..."` |
| Unknown tool | `"Error: Unknown tool: <name>"` |
| Missing required parameter | `"Error: Missing required parameter '<param>'"` |

All errors are logged to stderr with `[gmail_mcp]` prefix for debugging.

---

## Protocol

Implements JSON-RPC 2.0 over stdio:
- **Requests:** Read JSON from stdin (one per line)
- **Responses:** Write JSON to stdout
- **Logs:** Write to stderr (prefixed with `[gmail_mcp]`)

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