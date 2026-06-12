use anyhow::Result;
use serde_json::{Value, json};

use crate::core::agents;
use crate::core::tools::Tool;

pub struct ListAgents;

impl Tool for ListAgents {
    fn name(&self) -> &str { "list_agents" }
    fn category(&self) -> crate::core::tools::ToolCategory { crate::core::tools::ToolCategory::Introspection }

    fn description(&self) -> &str {
        "List sub-agents available to delegate work to. \
         Returns a JSON array of objects with id, name, description, and (optional) client. \
         Do NOT invoke the `main` agent."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn execute(&self, _args: Value) -> Result<String> {
        let mut list = agents::discover()?;
        // Exclude the root entry point and background system agents.
        list.retain(|a| a.id != "main" && !a.is_system_agent);

        let arr: Vec<Value> = list
            .into_iter()
            .map(|a| {
                let mut o = serde_json::Map::new();
                o.insert("id".into(),          Value::String(a.id));
                o.insert("name".into(),        Value::String(a.name));
                o.insert("description".into(), Value::String(a.description));
                if let Some(c) = a.client {
                    o.insert("client".into(), Value::String(c));
                }
                Value::Object(o)
            })
            .collect();

        Ok(serde_json::to_string_pretty(&arr)?)
    }
}
