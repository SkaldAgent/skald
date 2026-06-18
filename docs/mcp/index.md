# MCP (Model Context Protocol)

External tool integration via Model Context Protocol servers.

## Files

- [mcp.md](../mcp.md) — McpManager, transports (stdio/SSE), enable/disable, tool registration
- **Servers** (in `servers/` subdirectory):
  - [servers/gmail.md](servers/gmail.md) — Gmail read+modify MCP server (custom Python)
  - [servers/gcal.md](servers/gcal.md) — Google Calendar read-only MCP server (custom Python)
  - [servers/gmaps.md](servers/gmaps.md) — Google Maps transit/directions MCP server (custom Python)
  - [servers/whatsapp.md](servers/whatsapp.md) — WhatsApp read+send MCP server (custom Node.js)

See [../index.md#mcp-model-context-protocol](../index.md#mcp-model-context-protocol) for navigation.
