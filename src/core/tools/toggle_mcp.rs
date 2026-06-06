use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::core::mcp::McpManager;
use crate::core::tools::Tool;

pub struct ToggleMcp {
    mcp: Arc<McpManager>,
}

impl ToggleMcp {
    pub fn new(mcp: Arc<McpManager>) -> Self { Self { mcp } }
}

impl Tool for ToggleMcp {
    fn name(&self) -> &str { "toggle_mcp" }
    fn category(&self) -> crate::core::tools::ToolCategory { crate::core::tools::ToolCategory::Config }

    fn description(&self) -> &str {
        "Enable or disable an MCP server by name. Disabled servers won't connect on next restart. \
         Use `list_mcp` to see current server names and statuses. \
         NOTE: toggling does NOT start/stop a running server immediately — a restart is needed \
         for the change to take full effect."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the MCP server to toggle."
                },
                "enabled": {
                    "type": "boolean",
                    "description": "true to enable, false to disable."
                }
            },
            "required": ["name", "enabled"]
        })
    }

    fn execute(&self, args: Value) -> Result<String> {
        let name = args["name"].as_str()
            .ok_or_else(|| anyhow::anyhow!("toggle_mcp: missing required argument `name`"))?;
        let enabled = args["enabled"].as_bool()
            .ok_or_else(|| anyhow::anyhow!("toggle_mcp: missing required argument `enabled`"))?;

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.mcp.set_enabled(name, enabled))
        })?;

        Ok(format!(
            "MCP server '{}' is now {}. Note: a restart is required for the change to take effect on running servers.",
            name,
            if enabled { "enabled" } else { "disabled" }
        ))
    }
}