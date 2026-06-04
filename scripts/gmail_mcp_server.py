#!/usr/bin/env python3
"""Google Gmail MCP server (JSON-RPC 2.0 over stdio).

Provides read and write access to Gmail via the Gmail API v1.

Credentials are read from ./secrets/gmail_creds.json by default.
Override with GMAIL_CREDS_PATH env var.

Run scripts/gmail_oauth_setup.py first to generate the OAuth token.
"""

from __future__ import annotations

import base64
import json
import os
import sys
import threading
import time
from typing import Any

# Log to stderr so stdout stays clean for JSON-RPC.
def log(msg: str) -> None:
    print(f"[gmail_mcp] {msg}", file=sys.stderr, flush=True)

# Protects all stdout writes (main request-handling thread + poll thread).
_stdout_lock = threading.Lock()

# ── Push notifications ─────────────────────────────────────────────────────────

def _emit_notification(method: str, params: dict) -> None:
    """Write a JSON-RPC notification (no id) to stdout."""
    msg = json.dumps({"jsonrpc": "2.0", "method": method, "params": params})
    with _stdout_lock:
        sys.stdout.write(msg + "\n")
        sys.stdout.flush()


# State for incremental polling via the Gmail History API.
_last_history_id: str | None = None
_poll_thread: threading.Thread | None = None
_POLL_INTERVAL_SECS = 60


def _start_polling() -> None:
    """Build the service eagerly, record the initial historyId, start poll thread."""
    global _last_history_id, _poll_thread
    svc = _get_service()
    if svc is None:
        log("Gmail push polling disabled: service not available.")
        return
    try:
        profile = svc.users().getProfile(userId="me").execute()
        _last_history_id = str(profile.get("historyId", ""))
        log(f"Gmail polling started (historyId={_last_history_id}, interval={_POLL_INTERVAL_SECS}s).")
    except Exception as e:
        log(f"Failed to get initial historyId, polling disabled: {e}")
        return
    _poll_thread = threading.Thread(target=_poll_loop, daemon=True, name="gmail-poll")
    _poll_thread.start()


def _poll_loop() -> None:
    while True:
        time.sleep(_POLL_INTERVAL_SECS)
        _poll_once()


def _poll_once() -> None:
    global _last_history_id
    svc = _get_service()
    if svc is None or not _last_history_id:
        return
    try:
        result = svc.users().history().list(
            userId="me",
            startHistoryId=_last_history_id,
            labelId="INBOX",
            historyTypes=["messageAdded"],
        ).execute()

        # Always advance the cursor, even if no new messages.
        new_history_id = result.get("historyId")
        if new_history_id:
            _last_history_id = str(new_history_id)

        for record in result.get("history", []):
            for added in record.get("messagesAdded", []):
                msg_stub = added.get("message", {})
                if "INBOX" not in msg_stub.get("labelIds", []):
                    continue
                msg_id = msg_stub.get("id")
                if not msg_id:
                    continue
                _fetch_and_emit_email(svc, msg_id)

    except Exception as e:
        log(f"Gmail history poll error: {e}")


def _fetch_and_emit_email(svc: Any, msg_id: str) -> None:
    """Fetch metadata for a message and emit an event/new_email notification."""
    try:
        msg = svc.users().messages().get(
            userId="me",
            id=msg_id,
            format="metadata",
            metadataHeaders=["Subject", "From", "Date"],
        ).execute()
        headers = {h["name"]: h["value"] for h in msg.get("payload", {}).get("headers", [])}
        _emit_notification("event/new_email", {
            "message_id": msg_id,
            "thread_id":  msg.get("threadId"),
            "subject":    headers.get("Subject", "(no subject)"),
            "from":       headers.get("From", "?"),
            "date":       headers.get("Date", "?"),
            "snippet":    msg.get("snippet", "")[:300],
        })
        log(f"Notification emitted: new email {msg_id} from {headers.get('From', '?')!r}")
    except Exception as e:
        log(f"Failed to fetch metadata for message {msg_id}: {e}")


# ── Credentials / service ──────────────────────────────────────────────────────

_service = None
_init_error: str | None = None


def _build_service() -> Any:
    """Build and return a Gmail service object, or None on failure."""
    global _init_error
    try:
        from google.auth.transport.requests import Request
        from google.oauth2.credentials import Credentials
        from googleapiclient.discovery import build
    except ImportError as e:
        _init_error = f"Missing dependencies: {e}. Install google-api-python-client and google-auth."
        log(_init_error)
        return None

    creds_path = os.environ.get(
        "GMAIL_CREDS_PATH",
        os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "secrets", "gmail_creds.json"),
    )

    if not os.path.exists(creds_path):
        _init_error = (
            f"Credentials file not found at {creds_path}. "
            "Run scripts/gmail_oauth_setup.py first, or set GMAIL_CREDS_PATH."
        )
        log(_init_error)
        return None

    try:
        creds = Credentials.from_authorized_user_file(creds_path)
    except Exception as e:
        _init_error = f"Failed to load credentials from {creds_path}: {e}"
        log(_init_error)
        return None

    # Auto-refresh if expired.
    try:
        if not creds.valid:
            if creds.expired and creds.refresh_token:
                creds.refresh(Request())
                # Save updated token.
                with open(creds_path, "w") as f:
                    f.write(creds.to_json())
                log("Token refreshed and saved.")
            else:
                _init_error = "Credentials invalid and cannot be refreshed. Re-run scripts/gmail_oauth_setup.py."
                log(_init_error)
                return None
    except Exception as e:
        _init_error = f"Failed to refresh credentials: {e}"
        log(_init_error)
        return None

    try:
        service = build("gmail", "v1", credentials=creds)
    except Exception as e:
        _init_error = f"Failed to build Gmail service: {e}"
        log(_init_error)
        return None

    log(f"Gmail service built successfully (creds: {creds_path})")
    return service


def _get_service() -> Any:
    global _service
    if _service is None:
        _service = _build_service()
    return _service


# ── Helpers ────────────────────────────────────────────────────────────────────


def _decode_body(parts: Any) -> str:
    """Recursively extract text/plain body from MIME parts."""
    if isinstance(parts, list):
        for part in parts:
            mime_type = part.get("mimeType", "")
            if mime_type == "text/plain":
                data = part.get("body", {}).get("data", "")
                if data:
                    return _safe_b64decode(data)
            if "parts" in part:
                result = _decode_body(part["parts"])
                if result:
                    return result
    return ""


def _safe_b64decode(data: str) -> str:
    """Decode URL-safe base64 to string."""
    try:
        # Add padding if needed.
        padded = data + "=" * (4 - len(data) % 4) if len(data) % 4 else data
        decoded = base64.urlsafe_b64decode(padded)
        return decoded.decode("utf-8", errors="replace")
    except Exception:
        return "(unable to decode)"


def _format_datetime(ts_millis: int | None) -> str:
    """Format a unix timestamp in milliseconds to ISO-like string."""
    if ts_millis is None:
        return "?"
    return time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(ts_millis / 1000))


def _format_message_summary(msg: dict) -> str:
    """Format a message object (from list with metadata) into a summary line."""
    mid = msg.get("id", "?")
    headers = {h["name"]: h["value"] for h in msg.get("payload", {}).get("headers", [])}
    # When listing with metadata, headers might be elsewhere.
    payload = msg.get("payload", {})
    if not headers:
        headers = {h["name"]: h["value"] for h in payload.get("headers", [])}
    thread_id = msg.get("threadId", "?")
    subject = headers.get("Subject", "(no subject)")
    sender = headers.get("From", "?")
    date = headers.get("Date", "?")
    snippet = msg.get("snippet", "")[:80]
    return f"- {subject}\n  From: {sender} | Date: {date} | ID: {mid} | Thread: {thread_id}\n  {snippet}"


# ── Tool implementations ───────────────────────────────────────────────────────


def _list_messages(args: dict) -> str:
    """List messages with optional filters."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    query = args.get("query", "")
    max_results = min(args.get("max_results", 20), 50)
    label_ids = args.get("label_ids")

    params: dict = {
        "userId": "me",
        "maxResults": max_results,
    }
    if query:
        params["q"] = query
    if label_ids:
        if isinstance(label_ids, str):
            label_ids = [label_ids]
        params["labelIds"] = label_ids

    try:
        result = svc.users().messages().list(**params).execute()
    except Exception as e:
        return f"Error: Failed to list messages: {e}"

    items = result.get("messages", [])
    if not items:
        return "No messages found."

    # Fetch full metadata for each message.
    lines = [f"Messages ({len(items)} total):"]
    for entry in items:
        try:
            msg = svc.users().messages().get(userId="me", id=entry["id"], format="metadata", metadataHeaders=["Subject", "From", "Date"]).execute()
            lines.append(_format_message_summary(msg))
        except Exception as e:
            lines.append(f"- {entry['id']} (error fetching: {e})")

    # Add paging info.
    next_token = result.get("nextPageToken")
    if next_token:
        lines.append(f"\nMore results available. Use page_token='{next_token}' to get next page.")

    return "\n".join(lines)


def _get_message(args: dict) -> str:
    """Get full content of a single message by ID."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    msg_id = args.get("message_id")
    if not msg_id:
        return "Error: Missing required parameter 'message_id'."
    include_body = args.get("include_body", True)

    fmt = "full" if include_body else "metadata"

    try:
        msg = svc.users().messages().get(userId="me", id=msg_id, format=fmt).execute()
    except Exception as e:
        return f"Error: Failed to get message {msg_id}: {e}"

    payload = msg.get("payload", {})
    headers = {h["name"]: h["value"] for h in payload.get("headers", [])}

    lines = [
        f"ID: {msg.get('id', '?')}",
        f"Thread: {msg.get('threadId', '?')}",
        f"From: {headers.get('From', '?')}",
        f"To: {headers.get('To', '?')}",
        f"Date: {headers.get('Date', '?')}",
        f"Subject: {headers.get('Subject', '(no subject)')}",
        f"Labels: {', '.join(msg.get('labelIds', []))}",
    ]

    if include_body:
        body_text = _decode_body(payload.get("parts", []))
        if not body_text:
            # Try inline body.
            body_data = payload.get("body", {}).get("data", "")
            if body_data:
                body_text = _safe_b64decode(body_data)
        if body_text:
            lines.append("\n--- Body ---")
            # Truncate very long bodies.
            if len(body_text) > 10000:
                lines.append(body_text[:10000] + "\n... [truncated at 10000 chars]")
            else:
                lines.append(body_text)
        else:
            lines.append("\n(no text body found)")

    return "\n".join(lines)


def _get_thread(args: dict) -> str:
    """Get an entire thread (all messages in it)."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    thread_id = args.get("thread_id")
    if not thread_id:
        return "Error: Missing required parameter 'thread_id'."

    try:
        thread = svc.users().threads().get(userId="me", id=thread_id, format="metadata", metadataHeaders=["Subject", "From", "Date"]).execute()
    except Exception as e:
        return f"Error: Failed to get thread {thread_id}: {e}"

    messages = thread.get("messages", [])
    subject = ""
    lines = [f"Thread: {thread_id} ({len(messages)} messages)"]
    for i, msg in enumerate(messages, 1):
        headers = {h["name"]: h["value"] for h in msg.get("payload", {}).get("headers", [])}
        if not subject:
            subject = headers.get("Subject", "(no subject)")
        lines.append(f"\n[{i}] From: {headers.get('From', '?')} | Date: {headers.get('Date', '?')}")
        lines.append(f"    ID: {msg.get('id', '?')}")
        snippet = msg.get("snippet", "")
        if snippet:
            lines.append(f"    {snippet[:200]}")

    if subject:
        lines.insert(1, f"Subject: {subject}")

    return "\n".join(lines)


def _list_labels(args: dict) -> str:
    """List all labels/categories in the Gmail account."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    try:
        result = svc.users().labels().list(userId="me").execute()
    except Exception as e:
        return f"Error: Failed to list labels: {e}"

    items = result.get("labels", [])
    if not items:
        return "No labels found."

    lines = ["Labels:"]
    for lbl in items:
        lid = lbl.get("id", "?")
        name = lbl.get("name", "?")
        label_type = lbl.get("type", "?")
        msg_count = lbl.get("messagesTotal", "?")
        unread = lbl.get("messagesUnread", 0)
        lines.append(f"- {name} ({lid}) [{label_type}] — {msg_count} total, {unread} unread")

    return "\n".join(lines)


def _search_messages(args: dict) -> str:
    """Search messages with Gmail query syntax (alias for list)."""
    # This is the same as list_messages but defaults to more results.
    args.setdefault("max_results", 20)
    return _list_messages(args)


def _modify_message(args: dict) -> str:
    """Modify message labels (add/remove labels, mark read/archive/trash)."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    msg_id = args.get("message_id")
    if not msg_id:
        return "Error: Missing required parameter 'message_id'."

    add_labels = args.get("add_labels", [])
    remove_labels = args.get("remove_labels", [])

    if isinstance(add_labels, str):
        add_labels = [add_labels]
    if isinstance(remove_labels, str):
        remove_labels = [remove_labels]

    body: dict = {}
    if add_labels:
        body["addLabelIds"] = add_labels
    if remove_labels:
        body["removeLabelIds"] = remove_labels

    try:
        svc.users().messages().modify(userId="me", id=msg_id, body=body).execute()
    except Exception as e:
        return f"Error: Failed to modify message: {e}"

    changes = []
    if add_labels:
        changes.append(f"added labels: {add_labels}")
    if remove_labels:
        changes.append(f"removed labels: {remove_labels}")
    return f"Message {msg_id} modified: {'; '.join(changes)}"


def _send_message(args: dict) -> str:
    """Send an email message."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    to = args.get("to")
    subject = args.get("subject", "")
    body_text = args.get("body", "")
    cc = args.get("cc")
    bcc = args.get("bcc")
    in_reply_to = args.get("in_reply_to")
    thread_id = args.get("thread_id")

    if not to:
        return "Error: Missing required parameter 'to'."

    # Build headers in proper order: addressing → threading → MIME
    msg_lines = [f"To: {to}"]
    if cc:
        msg_lines.append(f"Cc: {cc}")
    if bcc:
        msg_lines.append(f"Bcc: {bcc}")
    msg_lines.append(f"Subject: {subject}")

    # RFC 2822 threading headers for in-thread reply
    if in_reply_to:
        msg_lines.append(f"In-Reply-To: <{in_reply_to}>")
        msg_lines.append(f"References: <{in_reply_to}>")

    msg_lines.append("Content-Type: text/plain; charset=utf-8")
    msg_lines.append("MIME-Version: 1.0")
    msg_lines.append("")
    msg_lines.append(body_text)

    raw_email = "\n".join(msg_lines)
    encoded = base64.urlsafe_b64encode(raw_email.encode("utf-8")).decode("utf-8")

    # Build API body — include threadId when replying in-thread
    api_body: dict = {"raw": encoded}
    if thread_id:
        api_body["threadId"] = thread_id

    try:
        sent = svc.users().messages().send(userId="me", body=api_body).execute()
    except Exception as e:
        return f"Error: Failed to send message: {e}"

    return f"✅ Message sent! ID: {sent.get('id', '?')}"


def _get_profile(args: dict) -> str:
    """Get Gmail profile info (email address, total/thread count)."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    try:
        profile = svc.users().getProfile(userId="me").execute()
    except Exception as e:
        return f"Error: Failed to get profile: {e}"

    return (
        f"Email: {profile.get('emailAddress', '?')}\n"
        f"Messages total: {profile.get('messagesTotal', '?')}\n"
        f"Threads total: {profile.get('threadsTotal', '?')}\n"
        f"History ID: {profile.get('historyId', '?')}"
    )


def _create_label(args: dict) -> str:
    """Create a new Gmail label."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    name = args.get("name")
    if not name:
        return "Error: Missing required parameter 'name'."

    label_list_visibility = args.get("label_list_visibility", "labelShow")
    message_list_visibility = args.get("message_list_visibility", "show")

    body = {
        "name": name,
        "labelListVisibility": label_list_visibility,
        "messageListVisibility": message_list_visibility,
    }

    try:
        result = svc.users().labels().create(userId="me", body=body).execute()
    except Exception as e:
        return f"Error: {e}"

    return f"Label '{result.get('name', name)}' created (ID: {result.get('id', '?')})"


def _collect_attachments(parts: Any, results: list) -> None:
    """Recursively collect attachment filenames and IDs from MIME parts."""
    if not parts:
        return
    for part in parts:
        filename = part.get("filename", "")
        attachment_id = part.get("body", {}).get("attachmentId", "")
        if filename and attachment_id:
            results.append({"filename": filename, "attachmentId": attachment_id})
        if "parts" in part:
            _collect_attachments(part["parts"], results)


def _download_attachments(args: dict) -> str:
    """Download all attachments from a Gmail message to a local folder."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    msg_id = args.get("message_id")
    if not msg_id:
        return "Error: Missing required parameter 'message_id'."

    folder = args.get("folder", "uploads/gmail_attachments/")

    try:
        msg = svc.users().messages().get(userId="me", id=msg_id, format="full").execute()
    except Exception as e:
        return f"Error: Failed to get message {msg_id}: {e}"

    payload = msg.get("payload", {})

    attachments: list = []
    _collect_attachments(payload.get("parts", []), attachments)

    if not attachments:
        return "No attachments found."

    os.makedirs(folder, exist_ok=True)

    saved = []
    for att in attachments:
        filename = att["filename"]
        attachment_id = att["attachmentId"]

        try:
            result = svc.users().messages().attachments().get(
                userId="me", messageId=msg_id, id=attachment_id
            ).execute()
        except Exception as e:
            saved.append(f"- {filename}: ERROR fetching attachment: {e}")
            continue

        data = result.get("data", "")
        if not data:
            saved.append(f"- {filename}: empty attachment data")
            continue

        try:
            file_data = base64.urlsafe_b64decode(data)
        except Exception as e:
            saved.append(f"- {filename}: ERROR decoding: {e}")
            continue

        safe_name = os.path.basename(filename)
        file_path = os.path.join(folder, safe_name)

        try:
            with open(file_path, "wb") as f:
                f.write(file_data)
        except Exception as e:
            saved.append(f"- {safe_name}: ERROR writing file: {e}")
            continue

        abs_path = os.path.abspath(file_path)
        size = len(file_data)
        saved.append(f"- {abs_path} ({size} bytes)")

    return "\n".join(["Attachments downloaded:"] + saved)

# ── JSON-RPC dispatch ──────────────────────────────────────────────────────────

TOOLS = [
    {
        "name": "list_messages",
        "description": "List Gmail messages with optional query filter and label filter. Uses Gmail search syntax (e.g. 'from:xxx', 'after:2024/1/1', 'is:unread'). Returns message summaries with subject, sender, date.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Gmail search query (e.g. 'from:john', 'is:unread', 'after:2024/01/01'). Leave empty for all recent messages.",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max messages to return (default 20, max 50).",
                },
                "label_ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Filter by label IDs (e.g. ['INBOX'], ['SENT'], ['STARRED']).",
                },
            },
        },
    },
    {
        "name": "get_message",
        "description": "Get full content of a Gmail message by ID, including body text.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message_id": {
                    "type": "string",
                    "description": "The Gmail message ID to retrieve.",
                },
                "include_body": {
                    "type": "boolean",
                    "description": "Whether to include the full body text (default true).",
                },
            },
            "required": ["message_id"],
        },
    },
    {
        "name": "get_thread",
        "description": "Get all messages in a thread by thread ID.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "thread_id": {
                    "type": "string",
                    "description": "The Gmail thread ID to retrieve.",
                },
            },
            "required": ["thread_id"],
        },
    },
    {
        "name": "list_labels",
        "description": "List all Gmail labels/folders/categories with message counts.",
        "inputSchema": {
            "type": "object",
            "properties": {},
        },
    },
    {
        "name": "search_messages",
        "description": "Search Gmail messages using Gmail search syntax. Alias for list_messages with default query support.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Gmail search query (e.g. 'from:john', 'is:unread', 'has:attachment').",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max messages to return (default 20, max 50).",
                },
                "label_ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Filter by label IDs (e.g. ['INBOX']).",
                },
            },
        },
    },
    {
        "name": "modify_message",
        "description": "Modify message labels (mark as read, archive, star, etc.). Use label IDs like 'UNREAD', 'STARRED', 'INBOX'.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message_id": {
                    "type": "string",
                    "description": "The Gmail message ID to modify.",
                },
                "add_labels": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Labels to add (e.g. ['STARRED'], ['LABEL_1']).",
                },
                "remove_labels": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Labels to remove (e.g. ['UNREAD'] to mark as read, ['INBOX'] to archive).",
                },
            },
            "required": ["message_id"],
        },
    },
    {
        "name": "send_message",
        "description": "Send an email via Gmail. Supports in-thread replies via optional in_reply_to and thread_id parameters.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient email address.",
                },
                "subject": {
                    "type": "string",
                    "description": "Email subject line.",
                },
                "body": {
                    "type": "string",
                    "description": "Plain text body of the email.",
                },
                "cc": {
                    "type": "string",
                    "description": "CC recipient email (optional).",
                },
                "bcc": {
                    "type": "string",
                    "description": "BCC recipient email (optional).",
                },
                "in_reply_to": {
                    "type": "string",
                    "description": "Message ID to reply to (adds In-Reply-To and References headers for proper threading).",
                },
                "thread_id": {
                    "type": "string",
                    "description": "Thread ID to attach the reply to (ensures the message appears in the correct Gmail thread).",
                },
            },
            "required": ["to", "subject", "body"],
        },
    },
    {
        "name": "get_profile",
        "description": "Get Gmail profile info: email address, total message/thread count.",
        "inputSchema": {
            "type": "object",
            "properties": {},
        },
    },
    {
        "name": "create_label",
        "description": "Create a new Gmail label/folder. Returns the label ID. Raises error if a label with the same name already exists.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the new label.",
                },
                "label_list_visibility": {
                    "type": "string",
                    "description": "Visibility in the label list: 'labelShow' (default), 'labelShowIfUnread', 'labelHide'.",
                    "default": "labelShow",
                },
                "message_list_visibility": {
                    "type": "string",
                    "description": "Visibility in the message list: 'show' (default) or 'hide'.",
                    "default": "show",
                },
            },
            "required": ["name"],
        },
    },
    {
        "name": "download_attachments",
        "description": "Download all attachments from a Gmail message to a local folder.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message_id": {
                    "type": "string",
                    "description": "The Gmail message ID to download attachments from.",
                },
                "folder": {
                    "type": "string",
                    "description": "Local folder to save attachments into (default: uploads/gmail_attachments/).",
                },
            },
            "required": ["message_id"],
        },
    },
]


def _ok(req_id: Any, result: Any) -> str:
    return json.dumps({"jsonrpc": "2.0", "id": req_id, "result": result})


def _text_result(req_id: Any, text: str, is_error: bool = False) -> str:
    payload: dict = {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {"content": [{"type": "text", "text": text}]},
    }
    if is_error:
        payload["result"]["isError"] = True
    return json.dumps(payload)


def handle_request(msg: dict) -> str | None:
    method = msg.get("method", "")
    req_id = msg.get("id")

    if method == "initialize":
        return _ok(req_id, {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {
                "name": "gmail",
                "version": "0.1.0",
            },
        })

    if method == "notifications/initialized":
        return None

    if method == "tools/list":
        return _ok(req_id, {"tools": TOOLS})

    if method == "tools/call":
        params = msg.get("params", {})
        tool_name = params.get("name", "")
        tool_args = params.get("arguments", {})

        try:
            if tool_name == "list_messages":
                text = _list_messages(tool_args)
            elif tool_name == "get_message":
                text = _get_message(tool_args)
            elif tool_name == "get_thread":
                text = _get_thread(tool_args)
            elif tool_name == "list_labels":
                text = _list_labels(tool_args)
            elif tool_name == "search_messages":
                text = _search_messages(tool_args)
            elif tool_name == "modify_message":
                text = _modify_message(tool_args)
            elif tool_name == "send_message":
                text = _send_message(tool_args)
            elif tool_name == "get_profile":
                text = _get_profile(tool_args)
            elif tool_name == "create_label":
                text = _create_label(tool_args)
            elif tool_name == "download_attachments":
                text = _download_attachments(tool_args)
            else:
                return _text_result(req_id, f"Unknown tool: {tool_name}", is_error=True)

            is_err = text.startswith("Error:")
            return _text_result(req_id, text, is_error=is_err)
        except Exception as e:
            log(f"Unhandled exception in tool '{tool_name}': {e}")
            return _text_result(req_id, f"Error: Internal error in tool '{tool_name}': {e}", is_error=True)

    # Unknown method.
    return json.dumps({
        "jsonrpc": "2.0",
        "id": req_id,
        "error": {"code": -32601, "message": f"Method not found: {method}"},
    })


# ── Main loop ──────────────────────────────────────────────────────────────────

def main() -> None:
    log("Starting Gmail MCP server")
    # Build the service eagerly and start the background polling thread.
    _start_polling()
    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError as e:
                log(f"Invalid JSON input: {e}")
                continue

            resp = handle_request(msg)
            if resp is not None:
                with _stdout_lock:
                    sys.stdout.write(resp + "\n")
                    sys.stdout.flush()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
