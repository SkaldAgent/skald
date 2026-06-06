use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::core::plugin::PluginManager;
use crate::core::tools::Tool;

pub struct ListPlugins(pub Arc<PluginManager>);

impl Tool for ListPlugins {
    fn name(&self) -> &str { "list_plugins" }
    fn category(&self) -> crate::core::tools::ToolCategory { crate::core::tools::ToolCategory::Introspection }

    fn description(&self) -> &str {
        "List all registered plugins with their id, name, description, \
         enabled flag (persisted), and running flag (live)."
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn execute(&self, _args: Value) -> Result<String> {
        let plugins = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.0.list())
        })?;
        Ok(serde_json::to_string_pretty(&plugins)?)
    }
}
