use anyhow::Result;
use serde_json::{Value, json};

use super::tool_names as tn;
use super::{Tool, ToolCategory};

pub struct ReadNotification;

impl Tool for ReadNotification {
    fn name(&self) -> &str {
        tn::READ_NOTIFICATION
    }

    fn description(&self) -> &str {
        "Read any pending notifications sent by background agents. Returns an array of notification strings."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn execute(&self, _args: Value) -> Result<String> {
        Ok("[]".to_string())
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Introspection
    }

    fn root_agent_only(&self) -> bool {
        true
    }
}
