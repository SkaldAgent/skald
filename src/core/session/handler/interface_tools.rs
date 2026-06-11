use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use serde_json::Value;

use crate::core::mcp::McpManager;
use crate::core::tools::Tool;

pub use core_api::interface_tool::{InterfaceTool, ToolFuture};

/// All configuration for a single agent run (root or sub-agent).
///
/// Passed by reference to `run_agent_turn` and `dispatch_call_agent`.
/// Callers build this once in `handle_message`; sub-agents receive a derived
/// config with an empty `interface_tools` (except `show_mcp_tools`) and fresh
/// `active_mcp_grants`.
pub struct AgentRunConfig {
    pub agent_id:     String,
    pub client_name:  String,
    /// Recursion depth: 0 = root agent, 1+ = sub-agent.
    pub depth:        i64,
    /// Global tool definitions (built-in tools only, no MCP).
    /// MCP tools are included dynamically in `all_tool_defs()` based on `active_mcp_grants`.
    pub base_tool_defs: Vec<Value>,
    /// Static extra context injected into the first (cacheable) system message.
    /// Example: Telegram HTML format instructions.  Should never contain
    /// per-turn data (timestamps, user-specific state) so the cached prefix
    /// remains byte-identical across turns.
    pub extra_system: Option<String>,
    /// Dynamic extra context injected as a separate system message AFTER the
    /// conversation history, just before the LLM generates its response.
    /// Example: Honcho long-term memory retrieved fresh every turn.
    /// Placing it at the tail keeps the stable prefix maximally cacheable
    /// while giving the model fresh user context at generation time.
    pub extra_system_dynamic: Option<String>,
    /// Short reminder injected as a trailing `system` message in the message list.
    pub tail_reminder: Option<String>,
    /// Named substitutions applied to the agent's system prompt at build time.
    /// Each entry replaces `__KEY__` sentinels produced by `agents::resolve_includes`.
    pub system_substitutions: HashMap<String, String>,
    /// Interface-specific tools.
    /// For sub-agents this contains only `show_mcp_tools`; all others are dropped.
    pub interface_tools: Vec<InterfaceTool>,
    /// Tools provided by the active memory backend (e.g. `memory_query`).
    pub memory_tools: Vec<Arc<dyn Tool>>,
    /// Image generation tools — present only when at least one provider is registered.
    pub image_tools: Vec<Arc<dyn Tool>>,
    /// MCP manager — used by `all_tool_defs()` to resolve which tools to include.
    pub mcp: Arc<McpManager>,
    /// Set of MCP server names currently granted (activated) for this agent run.
    ///
    /// - Root agents: pre-populated from `session_mcp_grants` DB at config-build time;
    ///   updated in-place by `show_mcp_tools`.
    /// - Sub-agents: starts empty; populated by `show_mcp_tools` (stack-scoped, no
    ///   session leak); deleted from DB when the stack frame terminates.
    ///
    /// `all_tool_defs()` re-reads this set on every call, so tools activated via
    /// `show_mcp_tools` in round N are available in round N+1 within the same turn.
    pub active_mcp_grants: Arc<RwLock<HashSet<String>>>,
    /// Tool names that are restricted to the root agent (depth == 0).
    /// Filtered out when deriving a sub-agent config via `for_sub_agent()`.
    pub root_only_tool_names: Vec<String>,
}

impl AgentRunConfig {
    /// Full tool list sent to the LLM on each round:
    ///   base tools  +  MCP tools for granted servers (dynamic)  +  memory tools  +  interface tools.
    ///
    /// MCP tools are re-queried every call so that a `show_mcp_tools` call in round N
    /// makes the tools visible in round N+1 without rebuilding the whole config.
    pub fn all_tool_defs(&self) -> Vec<Value> {
        let mut defs = self.base_tool_defs.clone();

        // Dynamic MCP: include tools for currently-granted servers.
        let granted: Vec<String> = self.active_mcp_grants
            .read()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default();
        if !granted.is_empty() {
            defs.extend(
                self.mcp.tools_for(&granted)
                    .iter()
                    .map(|t| t.to_openai_definition()),
            );
        }

        defs.extend(self.memory_tools.iter().map(|t| t.openai_definition()));
        defs.extend(self.image_tools.iter().map(|t| t.openai_definition()));
        defs.extend(self.interface_tools.iter().map(|t| t.definition.clone()));
        defs
    }

    /// Derives a config for a sub-agent:
    /// - Inherits base tools, memory tools, and MCP manager.
    /// - Starts with **empty** `active_mcp_grants` (sub-agents activate what they need).
    /// - Drops all interface tools (caller re-injects `show_mcp_tools` explicitly).
    /// - Increments depth.
    pub fn for_sub_agent(&self, agent_id: String, client_name: String) -> Self {
        let mut defs = self.base_tool_defs.clone();
        defs.retain(|def| {
            let name = def["function"]["name"].as_str().unwrap_or("");
            !self.root_only_tool_names.iter().any(|n| n == name)
        });

        Self {
            agent_id,
            client_name,
            depth:                self.depth + 1,
            base_tool_defs:       defs,
            extra_system:         None,
            extra_system_dynamic: None,
            tail_reminder:        None,
            system_substitutions: HashMap::new(),
            interface_tools:      vec![],
            memory_tools:         self.memory_tools.clone(),
            image_tools:          self.image_tools.clone(),
            mcp:                  Arc::clone(&self.mcp),
            active_mcp_grants:    Arc::new(RwLock::new(HashSet::new())),
            root_only_tool_names: self.root_only_tool_names.clone(),
        }
    }
}
