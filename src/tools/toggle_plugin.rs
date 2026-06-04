use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::plugin::PluginManager;
use crate::tools::Tool;

pub struct TogglePlugin(pub Arc<PluginManager>);

impl Tool for TogglePlugin {
    fn name(&self) -> &str { "toggle_plugin" }
    fn category(&self) -> crate::tools::ToolCategory { crate::tools::ToolCategory::Config }

    fn description(&self) -> &str {
        "Enable or disable a plugin by id. The change takes effect immediately \
         (the plugin is started or stopped at once) and is persisted to the DB \
         so it survives restarts. Use `list_plugins` to see available plugin ids."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Plugin id (e.g. \"telegram\")."
                },
                "enabled": {
                    "type": "boolean",
                    "description": "true to enable, false to disable."
                }
            },
            "required": ["id", "enabled"]
        })
    }

    fn execute(&self, args: Value) -> Result<String> {
        let id = args["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("toggle_plugin: missing required argument `id`"))?;
        let enabled = args["enabled"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("toggle_plugin: missing required argument `enabled`"))?;

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.0.toggle(id, enabled))
        })?;

        Ok(format!(
            "Plugin '{}' is now {}.",
            id,
            if enabled { "enabled and running" } else { "disabled and stopped" }
        ))
    }
}
