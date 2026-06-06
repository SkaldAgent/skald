use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::core::mcp::McpManager;
use crate::core::tools::{ToolDescriptionLength, ToolRegistry};
use crate::core::tools::tool_names as tn;

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name:        String,
    pub description: String,
    pub source:      String,
    pub server:      Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllTools {
    pub built_in: Vec<ToolInfo>,
    pub mcp:      Vec<ToolInfo>,
}

pub struct ToolCatalog {
    tools: Arc<ToolRegistry>,
    mcp:   Arc<McpManager>,
}

impl ToolCatalog {
    pub fn new(tools: Arc<ToolRegistry>, mcp: Arc<McpManager>) -> Self {
        Self { tools, mcp }
    }

    pub fn list_all(&self) -> AllTools {
        let mut built_in: Vec<ToolInfo> = self.tools
            .list_all()
            .into_iter()
            .map(|(name, description)| ToolInfo {
                name,
                description,
                source: "built-in".into(),
                server: None,
            })
            .collect();

        for (name, description) in Self::synthetic_tools() {
            built_in.push(ToolInfo {
                name:        (*name).to_string(),
                description: (*description).to_string(),
                source:      "built-in".into(),
                server:      None,
            });
        }

        built_in.sort_by(|a, b| a.name.cmp(&b.name));

        let mcp: Vec<ToolInfo> = self.mcp
            .tools()
            .into_iter()
            .map(|t| ToolInfo {
                name:        t.tool_id(),
                description: t.description,
                source:      "mcp".into(),
                server:      Some(t.server_name),
            })
            .collect();

        AllTools { built_in, mcp }
    }

    pub fn describe_call(&self, name: &str, args: &Value, length: ToolDescriptionLength) -> String {
        self.tools.describe_call(name, args, length)
    }

    fn synthetic_tools() -> &'static [(&'static str, &'static str)] {
        &[
            (tn::CALL_AGENT,             "Delegate a task to a specialised sub-agent."),
            (tn::UPDATE_SCRATCHPAD,      "Write a key-value note into the session scratchpad."),
            (tn::ASK_USER_CLARIFICATION, "Pause and ask the user a clarification question."),
        ]
    }
}
