# Google Maps MCP Server (gmaps)

## Overview

A Python MCP server providing **public-transit & mapping** capabilities via the Google Maps Platform APIs.

**Server name:** `gmaps`  
**Transport:** `stdio` (spawns `python3 scripts/gmaps_mcp_server.py`)  
**Location:** `scripts/gmaps_mcp_server.py`

---

## Tools

| Tool | Required params | Optional params | Description |
|------|----------------|-----------------|-------------|
| `maps_directions` | `origin`, `destination` | `mode`, `departure_time`, `transit_mode`, `transit_routing_preference`, `alternatives`, `language` | Step-by-step directions (transit, driving, walking, bicycling) |
| `maps_geocode` | `address` | `language`, `region` | Address / place name → coordinates + place_id |
| `maps_reverse_geocode` | `lat`, `lng` | `language` | Coordinates → formatted address |
| `maps_search_places` | *(at least one of `query`/`location`)* | `radius`, `type`, `language` | Find nearby stations, stops, POIs |
| `maps_distance_matrix` | `origins`, `destinations` | `mode`, `language` | Travel time & distance between multiple points |

---

## Authentication

### API Key

Unlike Gmail/Calendar (OAuth), Google Maps uses a **plain API key**.

**Priority order:**

1. Environment variable `GOOGLE_MAPS_API_KEY`
2. File `secrets/gmaps_api_key.txt` (first non-empty line)

The `secrets/` directory is in `.gitignore` — the key will not be committed.

### Required Google Cloud APIs

Enable all four in the [Google Cloud Console](https://console.cloud.google.com/apis/library):

| API | Used by |
|-----|---------|
| **Directions API** | `maps_directions` |
| **Geocoding API** | `maps_geocode`, `maps_reverse_geocode` |
| **Places API** | `maps_search_places` |
| **Distance Matrix API** | `maps_distance_matrix` |

---

## Setup

### 1. Create API Key

1. Go to [Google Cloud Console → Credentials](https://console.cloud.google.com/apis/credentials)
2. Click **Create credentials → API key**
3. (Recommended) Restrict the key to the four APIs above

### 2. Save the key

```bash
echo "YOUR_API_KEY_HERE" > secrets/gmaps_api_key.txt
```

Or set the environment variable in your shell/`run.sh`:

```bash
export GOOGLE_MAPS_API_KEY=AIza...
```

### 3. Install the Python dependency

```bash
.venv/bin/pip install googlemaps
# or, if you re-run run.sh, it installs requirements.txt automatically
```

`googlemaps` is already listed in `requirements.txt`.

### 4. Register the server with the agent

Ask the agent:
```
register_mcp(
  name="gmaps",
  transport="stdio",
  command="python3",
  args=["scripts/gmaps_mcp_server.py"]
)
```

---

## Usage Examples

### Transit directions home (now)

```
mcp__gmaps__maps_directions(
  origin="Piazza del Duomo, Milano",
  destination="casa mia",   ← or the real address saved in agent memory
  mode="transit"
)
```

### Prefer train, fewer transfers

```
mcp__gmaps__maps_directions(
  origin="current location",
  destination="Via Roma 1, Torino",
  mode="transit",
  transit_mode="train",
  transit_routing_preference="fewer_transfers"
)
```

### Find the nearest metro station

```
mcp__gmaps__maps_search_places(
  query="metro",
  location="Piazza Garibaldi, Napoli",
  radius=500,
  type="subway_station"
)
```

### How long does it take from A to B?

```
mcp__gmaps__maps_distance_matrix(
  origins="Stazione Centrale, Milano",
  destinations="Aeroporto Malpensa",
  mode="transit"
)
```

---

## Enable / Disable

```
toggle_mcp(name="gmaps", enabled=false)   # disable
toggle_mcp(name="gmaps", enabled=true)    # re-enable
restart                                    # required for changes to take effect
```

---

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `googlemaps` | latest | Google Maps Platform Python client |

---

## Parameter notes

### Coordinates format

Whenever a parameter accepts coordinates, pass them as a **`"latitude,longitude"` decimal string with no spaces**, e.g. `"45.4654,9.1866"`. Never pass an array or separate fields.

### `departure_time`

Must be the **literal string `"now"`** or an **ISO 8601 datetime string with timezone offset**, e.g. `"2025-06-15T08:30:00+02:00"`. Never pass a Unix timestamp integer.

### `transit_mode`

Restricts results to a specific vehicle: `"train"` = intercity/regional rail, `"subway"` = metro, `"tram"` = trams, `"bus"` = buses, `"rail"` = any rail. Omit to allow any vehicle.

---

## Error Handling

| Error | Response |
|-------|----------|
| API key not found | `"Error: Google Maps API key not found. Set GOOGLE_MAPS_API_KEY…"` |
| `googlemaps` not installed | `"Error: Missing dependency: No module named 'googlemaps'. Run: pip install googlemaps"` |
| No route found | `"No routes found from '…' to '…'."` |
| API call failure | `"Error: Directions API call failed: …"` |
| Missing required param | `"Error: Missing required parameter '…'"` |

All errors are logged to stderr with `[gmaps_mcp]` prefix.

---

## Protocol

Implements JSON-RPC 2.0 over stdio (same as gcal and gmail servers):

- **Requests:** read from stdin, one JSON object per line
- **Responses:** written to stdout
- **Logs:** stderr only, prefixed `[gmaps_mcp]`

Supported methods: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`.

---

## When to Update This File

- New tools added
- Auth mechanism changes (e.g. OAuth migration)
- New transport option added
- Error cases change
