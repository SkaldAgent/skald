#!/usr/bin/env python3
"""Google Calendar MCP server (JSON-RPC 2.0 over stdio).

Capabilities: list_calendars, list_events, get_event (read-only),
              create_event, update_event, delete_event, respond_to_event (write).

Credentials are read from ./secrets/google_creds.json by default.
Override with GOOGLE_CREDS_PATH env var.

Required OAuth scopes:
  https://www.googleapis.com/auth/calendar
  (or https://www.googleapis.com/auth/calendar.events for events-only)

Run scripts/gcal_oauth_setup.py to (re-)authenticate.
"""

from __future__ import annotations

import json
import os
import sys
import threading
import time
from datetime import datetime, timezone
from typing import Any

# Log to stderr so stdout stays clean for JSON-RPC.
def log(msg: str) -> None:
    print(f"[gcal_mcp] {msg}", file=sys.stderr, flush=True)

# Protects all stdout writes (main thread + poll thread).
_stdout_lock = threading.Lock()

# ── Push notifications ─────────────────────────────────────────────────────────

def _emit_notification(method: str, params: dict) -> None:
    """Write a JSON-RPC notification (no id) to stdout."""
    msg = json.dumps({"jsonrpc": "2.0", "method": method, "params": params})
    with _stdout_lock:
        sys.stdout.write(msg + "\n")
        sys.stdout.flush()


# ISO-8601 UTC timestamp of when we last polled.
# We emit events whose `created` field is >= this value.
_last_poll_at: str | None = None
_poll_thread: threading.Thread | None = None
_POLL_INTERVAL_SECS = 300  # 5 minutes


def _utc_now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _start_polling() -> None:
    """Build service eagerly, record start time, launch poll thread."""
    global _last_poll_at, _poll_thread
    svc = _get_service()
    if svc is None:
        log("GCal push polling disabled: service not available.")
        return
    _last_poll_at = _utc_now_iso()
    log(f"GCal polling started (tracking events created after {_last_poll_at}, interval={_POLL_INTERVAL_SECS}s).")
    _poll_thread = threading.Thread(target=_poll_loop, daemon=True, name="gcal-poll")
    _poll_thread.start()


def _poll_loop() -> None:
    while True:
        time.sleep(_POLL_INTERVAL_SECS)
        _poll_once()


def _poll_once() -> None:
    global _last_poll_at
    svc = _get_service()
    if svc is None or _last_poll_at is None:
        return

    since = _last_poll_at
    _last_poll_at = _utc_now_iso()  # advance cursor before the call (safe: we only advance)

    try:
        result = svc.events().list(
            calendarId="primary",
            updatedMin=since,
            singleEvents=True,
            orderBy="updated",
            maxResults=50,
        ).execute()

        for ev in result.get("items", []):
            # Emit only events that were newly *created* in this window (not just modified).
            created = ev.get("created", "")
            if created < since:
                continue
            start = ev.get("start") or {}
            end   = ev.get("end")   or {}
            _emit_notification("event/new_calendar_event", {
                "event_id":    ev.get("id"),
                "summary":     ev.get("summary", "(no title)"),
                "start":       start.get("dateTime") or start.get("date"),
                "end":         end.get("dateTime")   or end.get("date"),
                "location":    ev.get("location"),
                "description": (ev.get("description") or "")[:500],
                "html_link":   ev.get("htmlLink"),
                "created":     created,
            })
            log(f"Notification emitted: new calendar event {ev.get('id')!r} — {ev.get('summary')!r}")

    except Exception as e:
        log(f"GCal poll error: {e}")


# ── Credentials / service ──────────────────────────────────────────────────────

_service = None
_init_error: str | None = None


def _build_service() -> Any:
    """Build and return a Google Calendar service object, or None on failure."""
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
        "GOOGLE_CREDS_PATH",
        os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "secrets", "google_creds.json"),
    )

    if not os.path.exists(creds_path):
        _init_error = (
            f"Credentials file not found at {creds_path}. "
            "Run scripts/gcal_oauth_setup.py to authenticate, or set GOOGLE_CREDS_PATH."
        )
        log(_init_error)
        return None

    try:
        creds = Credentials.from_authorized_user_file(creds_path)
    except Exception as e:
        _init_error = f"Failed to load credentials from {creds_path}: {e}"
        log(_init_error)
        return None

    # Refresh expired token automatically.
    if creds.expired and creds.refresh_token:
        try:
            from google.auth.transport.requests import Request
            creds.refresh(Request())
            with open(creds_path, "w") as f:
                f.write(creds.to_json())
            log("Token refreshed and saved.")
        except Exception as e:
            log(f"Token refresh failed: {e}")

    try:
        service = build("calendar", "v3", credentials=creds)
    except Exception as e:
        _init_error = f"Failed to build Calendar service: {e}"
        log(_init_error)
        return None

    log(f"Calendar service built successfully (creds: {creds_path})")
    return service


def _get_service() -> Any:
    global _service
    if _service is None:
        _service = _build_service()
    return _service


# ── Tool implementations ───────────────────────────────────────────────────────


def _list_calendars(args: dict | None = None) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    try:
        result = svc.calendarList().list().execute()
    except Exception as e:
        return f"Error: Failed to list calendars: {e}"

    items = result.get("items", [])
    if not items:
        return "No calendars found."

    lines = []
    for cal in items:
        cal_id = cal.get("id", "?")
        summary = cal.get("summary", "(no name)")
        primary = " [PRIMARY]" if cal.get("primary", False) else ""
        lines.append(f"- {summary}{primary}  (id: {cal_id})")
    return "\n".join(lines)


def _list_events(args: dict) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    calendar_id = args.get("calendar_id", "primary")
    max_results = args.get("max_results", 100)
    full_text = args.get("full_text")
    time_zone = args.get("time_zone", "Europe/Rome")

    # Accept both "time_min"/"time_max" (preferred, mirrors GCal API) and the
    # legacy "start_time"/"end_time" aliases so old callers keep working.
    # Default time_min to now so we never return stale past events by accident.
    start_time = args.get("time_min") or args.get("start_time") or _utc_now_iso()
    end_time   = args.get("time_max") or args.get("end_time")

    params: dict = {
        "calendarId": calendar_id,
        "maxResults": min(max(int(max_results), 1), 250),
        "timeZone": time_zone,
        "timeMin": start_time,
        "singleEvents": True,   # expand recurring events into individual instances
        "orderBy": "startTime", # chronological order (requires singleEvents=True)
    }

    if end_time:
        params["timeMax"] = end_time

    if full_text:
        params["q"] = full_text

    try:
        result = svc.events().list(**params).execute()
    except Exception as e:
        return f"Error: Failed to list events: {e}"

    items = result.get("items", [])
    if not items:
        return "No events found."

    lines = [f"Events ({len(items)} total):"]
    for ev in items:
        summary = ev.get("summary", "(no title)")
        start = ev.get("start", {})
        end = ev.get("end", {})
        start_str = start.get("dateTime") or start.get("date") or "?"
        end_str = end.get("dateTime") or end.get("date") or "?"
        ev_id = ev.get("id", "?")
        lines.append(f"- {summary}")
        lines.append(f"  When: {start_str} → {end_str}")
        lines.append(f"  ID: {ev_id}")
    return "\n".join(lines)


def _get_event(args: dict) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    event_id = args.get("event_id")
    if not event_id:
        return "Error: Missing required parameter 'event_id'."

    calendar_id = args.get("calendar_id", "primary")

    try:
        result = svc.events().get(calendarId=calendar_id, eventId=event_id).execute()
    except Exception as e:
        return f"Error: Failed to get event: {e}"

    summary = result.get("summary", "(no title)")
    description = result.get("description", "(no description)")
    start = result.get("start", {})
    end = result.get("end", {})
    start_str = start.get("dateTime") or start.get("date") or "?"
    end_str = end.get("dateTime") or end.get("date") or "?"
    location = result.get("location", "(no location)")
    attendees = result.get("attendees", [])

    lines = [
        f"Event: {summary}",
        f"  ID: {event_id}",
        f"  When: {start_str} → {end_str}",
        f"  Location: {location}",
        f"  Description: {description}",
    ]
    if attendees:
        lines.append("  Attendees:")
        for a in attendees:
            email = a.get("email", "?")
            status = a.get("responseStatus", "?")
            lines.append(f"    - {email} ({status})")
    return "\n".join(lines)


def _create_event(args: dict) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    summary = args.get("summary")
    if not summary:
        return "Error: Missing required parameter 'summary'."

    start = args.get("start")
    end = args.get("end")
    if not start or not end:
        return "Error: Missing required parameters 'start' and/or 'end'."

    calendar_id = args.get("calendar_id", "primary")

    # Build start/end objects: support dateTime (with timezone) or date (all-day).
    def _time_obj(value: str, time_zone: str) -> dict:
        if "T" in value:
            return {"dateTime": value, "timeZone": time_zone}
        return {"date": value}

    time_zone = args.get("time_zone", "Europe/Rome")

    body: dict = {
        "summary": summary,
        "start": _time_obj(start, time_zone),
        "end": _time_obj(end, time_zone),
    }

    if args.get("description"):
        body["description"] = args["description"]
    if args.get("location"):
        body["location"] = args["location"]

    attendees_raw = args.get("attendees", [])
    if attendees_raw:
        body["attendees"] = [{"email": e} for e in attendees_raw]

    if args.get("recurrence"):
        body["recurrence"] = args["recurrence"]  # e.g. ["RRULE:FREQ=WEEKLY;COUNT=5"]

    reminders_raw = args.get("reminders")
    if reminders_raw is not None:
        # Accept both list-of-dicts and list-of-minutes (popup only).
        overrides = []
        for r in reminders_raw:
            if isinstance(r, dict):
                overrides.append({"method": r.get("method", "popup"), "minutes": int(r.get("minutes", 10))})
            else:
                overrides.append({"method": "popup", "minutes": int(r)})
        body["reminders"] = {"useDefault": False, "overrides": overrides}

    try:
        result = svc.events().insert(calendarId=calendar_id, body=body).execute()
    except Exception as e:
        return f"Error: Failed to create event: {e}"

    ev_id = result.get("id", "?")
    link = result.get("htmlLink", "")
    return f"Event created: {summary}\n  ID: {ev_id}\n  Link: {link}"


def _update_event(args: dict) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    event_id = args.get("event_id")
    if not event_id:
        return "Error: Missing required parameter 'event_id'."

    calendar_id = args.get("calendar_id", "primary")
    time_zone = args.get("time_zone", "Europe/Rome")

    # Fetch the existing event so we can patch only what changed.
    try:
        existing = svc.events().get(calendarId=calendar_id, eventId=event_id).execute()
    except Exception as e:
        return f"Error: Failed to fetch event for update: {e}"

    def _time_obj(value: str, tz: str) -> dict:
        if "T" in value:
            return {"dateTime": value, "timeZone": tz}
        return {"date": value}

    if args.get("summary"):
        existing["summary"] = args["summary"]
    if args.get("description") is not None:
        existing["description"] = args["description"]
    if args.get("location") is not None:
        existing["location"] = args["location"]
    if args.get("start"):
        existing["start"] = _time_obj(args["start"], time_zone)
    if args.get("end"):
        existing["end"] = _time_obj(args["end"], time_zone)
    if args.get("attendees") is not None:
        existing["attendees"] = [{"email": e} for e in args["attendees"]]
    if args.get("reminders") is not None:
        overrides = []
        for r in args["reminders"]:
            if isinstance(r, dict):
                overrides.append({"method": r.get("method", "popup"), "minutes": int(r.get("minutes", 10))})
            else:
                overrides.append({"method": "popup", "minutes": int(r)})
        existing["reminders"] = {"useDefault": False, "overrides": overrides}

    try:
        result = svc.events().update(calendarId=calendar_id, eventId=event_id, body=existing).execute()
    except Exception as e:
        return f"Error: Failed to update event: {e}"

    summary = result.get("summary", event_id)
    return f"Event updated: {summary}\n  ID: {event_id}"


def _delete_event(args: dict) -> str:
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    event_id = args.get("event_id")
    if not event_id:
        return "Error: Missing required parameter 'event_id'."

    calendar_id = args.get("calendar_id", "primary")

    try:
        svc.events().delete(calendarId=calendar_id, eventId=event_id).execute()
    except Exception as e:
        return f"Error: Failed to delete event: {e}"

    return f"Event {event_id} deleted successfully."


def _respond_to_event(args: dict) -> str:
    """RSVP to an event by updating the self attendee status."""
    svc = _get_service()
    if svc is None:
        return f"Error: {_init_error}"

    event_id = args.get("event_id")
    if not event_id:
        return "Error: Missing required parameter 'event_id'."

    response = args.get("response", "").lower()
    valid = {"accepted", "declined", "tentative", "needsAction"}
    if response not in valid:
        return f"Error: 'response' must be one of: {', '.join(sorted(valid))}."

    calendar_id = args.get("calendar_id", "primary")

    try:
        existing = svc.events().get(calendarId=calendar_id, eventId=event_id).execute()
    except Exception as e:
        return f"Error: Failed to fetch event: {e}"

    attendees = existing.get("attendees", [])
    updated = False
    for a in attendees:
        if a.get("self"):
            a["responseStatus"] = response
            updated = True
            break

    if not updated:
        # No self attendee found — add one.
        # We need the authenticated user's email; fetch it from settings.
        try:
            cal_info = svc.calendars().get(calendarId="primary").execute()
            self_email = cal_info.get("id", "")
        except Exception:
            self_email = ""
        if self_email:
            attendees.append({"email": self_email, "self": True, "responseStatus": response})
            existing["attendees"] = attendees
        else:
            return "Error: Could not determine your email to set RSVP."

    try:
        result = svc.events().patch(
            calendarId=calendar_id,
            eventId=event_id,
            body={"attendees": existing["attendees"]},
            sendUpdates="none",
        ).execute()
    except Exception as e:
        return f"Error: Failed to update RSVP: {e}"

    summary = result.get("summary", event_id)
    return f"RSVP set to '{response}' for event: {summary}"


# ── Tool manifest ──────────────────────────────────────────────────────────────

TOOLS = [
    # ── Read-only ──────────────────────────────────────────────────────────────
    {
        "name": "list_calendars",
        "description": "Lists all calendars accessible to the authenticated user.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "list_events",
        "description": (
            "Lists calendar events from a given calendar, ordered chronologically. "
            "If time_min is omitted, defaults to NOW (current UTC time) — so you never get past events by accident. "
            "If time_max is omitted, the API returns events from time_min onward up to max_results. "
            "Always pass time_min and time_max explicitly when you need a specific range."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
                "time_min": {
                    "type": "string",
                    "description": "ISO 8601 lower bound (inclusive), e.g. '2025-01-01T00:00:00+01:00'. Also accepted as 'start_time'.",
                },
                "time_max": {
                    "type": "string",
                    "description": "ISO 8601 upper bound (exclusive). Also accepted as 'end_time'.",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max events to return. Default 100.",
                },
                "full_text": {
                    "type": "string",
                    "description": "Free-text search across title, description, location, attendees.",
                },
                "time_zone": {
                    "type": "string",
                    "description": "IANA timezone. Default 'Europe/Rome'.",
                },
            },
        },
    },
    {
        "name": "get_event",
        "description": "Returns a single calendar event by ID.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "The ID of the event to retrieve.",
                },
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
            },
            "required": ["event_id"],
        },
    },
    # ── Write ──────────────────────────────────────────────────────────────────
    {
        "name": "create_event",
        "description": (
            "Creates a new event in the specified calendar. "
            "Use ISO 8601 for start/end (e.g. '2025-06-15T10:00:00' for timed events, "
            "'2025-06-15' for all-day events)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Title / subject of the event.",
                },
                "start": {
                    "type": "string",
                    "description": "Start datetime (ISO 8601) or date (YYYY-MM-DD for all-day).",
                },
                "end": {
                    "type": "string",
                    "description": "End datetime (ISO 8601) or date (YYYY-MM-DD for all-day).",
                },
                "description": {
                    "type": "string",
                    "description": "Optional longer description / notes.",
                },
                "location": {
                    "type": "string",
                    "description": "Optional location string.",
                },
                "attendees": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of attendee email addresses.",
                },
                "recurrence": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional RRULE strings, e.g. ['RRULE:FREQ=WEEKLY;COUNT=4'].",
                },
                "time_zone": {
                    "type": "string",
                    "description": "IANA timezone for start/end. Default 'Europe/Rome'.",
                },
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
                "reminders": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {"type": "integer", "description": "Minutes before event for a popup reminder."},
                            {
                                "type": "object",
                                "properties": {
                                    "method": {"type": "string", "enum": ["popup", "email"], "description": "Reminder method. Default 'popup'."},
                                    "minutes": {"type": "integer", "description": "Minutes before the event."},
                                },
                            },
                        ],
                    },
                    "description": "Optional custom reminders. Pass integers for popup reminders (e.g. [10, 30, 60]) or dicts for full control ([{'method': 'popup', 'minutes': 10}]). Overrides calendar defaults.",
                },
            },
            "required": ["summary", "start", "end"],
        },
    },
    {
        "name": "update_event",
        "description": (
            "Updates an existing event. Only fields provided are changed; "
            "omitted fields keep their current values."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "ID of the event to update.",
                },
                "summary": {"type": "string", "description": "New title."},
                "start": {"type": "string", "description": "New start (ISO 8601 or YYYY-MM-DD)."},
                "end": {"type": "string", "description": "New end (ISO 8601 or YYYY-MM-DD)."},
                "description": {"type": "string", "description": "New description."},
                "location": {"type": "string", "description": "New location."},
                "attendees": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Replacement attendee list (emails). Replaces all existing attendees.",
                },
                "time_zone": {
                    "type": "string",
                    "description": "IANA timezone for start/end. Default 'Europe/Rome'.",
                },
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
                "reminders": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {"type": "integer", "description": "Minutes before event for a popup reminder."},
                            {
                                "type": "object",
                                "properties": {
                                    "method": {"type": "string", "enum": ["popup", "email"], "description": "Reminder method. Default 'popup'."},
                                    "minutes": {"type": "integer", "description": "Minutes before the event."},
                                },
                            },
                        ],
                    },
                    "description": "Optional custom reminders. Pass integers for popup reminders (e.g. [10, 30, 60]) or dicts for full control ([{'method': 'popup', 'minutes': 10}]). Overrides calendar defaults.",
                },
            },
            "required": ["event_id"],
        },
    },
    {
        "name": "delete_event",
        "description": "Permanently deletes a calendar event.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "ID of the event to delete.",
                },
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
            },
            "required": ["event_id"],
        },
    },
    {
        "name": "respond_to_event",
        "description": "Set your RSVP / attendance response for a calendar event.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "ID of the event.",
                },
                "response": {
                    "type": "string",
                    "enum": ["accepted", "declined", "tentative", "needsAction"],
                    "description": "Your response: accepted, declined, tentative, or needsAction.",
                },
                "calendar_id": {
                    "type": "string",
                    "description": "Calendar ID. Defaults to 'primary'.",
                },
            },
            "required": ["event_id", "response"],
        },
    },
]


# ── JSON-RPC dispatch ──────────────────────────────────────────────────────────

TOOL_DISPATCH = {
    "list_calendars": _list_calendars,
    "list_events": _list_events,
    "get_event": _get_event,
    "create_event": _create_event,
    "update_event": _update_event,
    "delete_event": _delete_event,
    "respond_to_event": _respond_to_event,
}


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
                "name": "gcal",
                "version": "0.2.0",
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

        handler = TOOL_DISPATCH.get(tool_name)
        if handler is None:
            return _text_result(req_id, f"Unknown tool: {tool_name}", is_error=True)

        try:
            text = handler(tool_args)
            is_err = text.startswith("Error:")
            return _text_result(req_id, text, is_error=is_err)
        except Exception as e:
            log(f"Unhandled exception in tool '{tool_name}': {e}")
            return _text_result(req_id, f"Error: Internal error in tool '{tool_name}': {e}", is_error=True)

    return json.dumps({
        "jsonrpc": "2.0",
        "id": req_id,
        "error": {"code": -32601, "message": f"Method not found: {method}"},
    })


# ── Main loop ──────────────────────────────────────────────────────────────────

def main() -> None:
    log("Starting gcal MCP server (read + write)")
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
