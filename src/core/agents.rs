use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

use crate::config::LlmStrength;

const AGENTS_DIR: &str = "agents";

#[derive(Deserialize)]
struct RawMeta {
    name:          String,
    description:   String,
    #[serde(default)]
    inject_memory: Vec<String>,
    #[serde(default)]
    client:        Option<String>,
    #[serde(default)]
    scope:         Option<String>,
    #[serde(default)]
    strength:      Option<LlmStrength>,
    #[serde(default)]
    allow_tools:   Option<Vec<String>>,
    #[serde(default)]
    is_system_agent: bool,
    #[serde(default)]
    icon:          Option<String>,
    #[serde(default)]
    run_context:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMeta {
    pub id:            String,
    pub name:          String,
    pub description:   String,
    #[serde(default)]
    pub inject_memory: Vec<String>,
    /// Preferred LLM client name (must exist in the DB, configured via the web app).
    /// If unset, the sub-agent inherits the caller's client.
    #[serde(default)]
    pub client:        Option<String>,
    /// Task domain this agent operates in (e.g. "coding", "reasoning").
    /// Used by AUTO client selection to find a matching LLM.
    #[serde(default)]
    pub scope:         Option<String>,
    /// Minimum LLM capability required to run this agent reliably.
    /// AUTO selection skips clients weaker than this threshold.
    #[serde(default)]
    pub strength:      Option<LlmStrength>,
    /// Whitelist of system tool names this agent may use.
    /// When present, only these tools (plus all MCP tools) are passed to the LLM.
    /// When absent, all tools are available (default behaviour).
    #[serde(default)]
    pub allow_tools:   Option<Vec<String>>,
    /// When true, this is a background system agent (e.g. TIC).
    /// It is excluded from `list_agents` output and the AGENTS_LIST injection,
    /// so the main agent cannot see or call it.
    #[serde(default)]
    pub is_system_agent: bool,
    /// Path to the agent's icon image file (relative to the agent's directory).
    /// Defaults to None if no icon is configured.
    #[serde(default)]
    pub icon: Option<String>,
    /// Default RunContext id for sessions started with this agent.
    /// When `None`, the session uses the built-in "default" run_context.
    #[serde(default)]
    pub run_context: Option<String>,
}

/// Scan `agents/` and return metadata for every agent that has both
/// `meta.json` and `AGENT.md`. Skips the `common/` directory.
pub fn discover() -> Result<Vec<AgentMeta>> {
    let mut agents = Vec::new();

    let dir = std::fs::read_dir(AGENTS_DIR)
        .with_context(|| format!("Failed to read agents directory '{AGENTS_DIR}'"))?;

    for entry in dir {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() { continue; }

        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.is_empty() && n != "common" => n.to_string(),
            _ => continue,
        };

        let meta_path   = path.join("meta.json");
        let system_path = path.join("AGENT.md");
        if !meta_path.exists() || !system_path.exists() {
            warn!(agent_id = %id, "skipping agent: missing meta.json or AGENT.md");
            continue;
        }

        let raw: RawMeta = serde_json::from_str(
            &std::fs::read_to_string(&meta_path)
                .with_context(|| format!("Failed to read {}", meta_path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", meta_path.display()))?;

        let meta = AgentMeta {
            id,
            name:            raw.name,
            description:     raw.description,
            inject_memory:   raw.inject_memory,
            client:          raw.client,
            scope:           raw.scope,
            strength:        raw.strength,
            allow_tools:     raw.allow_tools,
            is_system_agent: raw.is_system_agent,
            icon:            raw.icon,
            run_context:     raw.run_context,
        };
        trace!(agent_id = %meta.id, client = ?meta.client, scope = ?meta.scope, strength = ?meta.strength, "agent meta loaded");
        debug!(agent_id = %meta.id, name = %meta.name, "agent discovered");
        agents.push(meta);
    }

    agents.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(agents)
}

/// Load metadata for a single agent (reads its `meta.json`).
pub fn load_meta(agent_id: &str) -> Result<AgentMeta> {
    let path = format!("{AGENTS_DIR}/{agent_id}/meta.json");
    let raw_str = std::fs::read_to_string(&path)
        .with_context(|| format!("Agent '{agent_id}': meta.json not found at '{path}'"))?;

    let raw: RawMeta = serde_json::from_str(&raw_str)
        .with_context(|| format!("Agent '{agent_id}': failed to parse meta.json"))?;

    Ok(AgentMeta {
        id:              agent_id.to_string(),
        name:            raw.name,
        description:     raw.description,
        inject_memory:   raw.inject_memory,
        client:          raw.client,
        scope:           raw.scope,
        strength:        raw.strength,
        allow_tools:     raw.allow_tools,
        is_system_agent: raw.is_system_agent,
        icon:            raw.icon,
        run_context:     raw.run_context,
    })
}

/// Load and resolve the system prompt for `agent_id` from disk.
/// Called at request time so edits to `.md` files take effect without restart.
pub fn load_prompt(agent_id: &str) -> Result<String> {
    let path = format!("{AGENTS_DIR}/{agent_id}/AGENT.md");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Agent '{agent_id}': AGENT.md not found at '{path}'"))?;
    resolve_includes(&content)
}

fn resolve_includes(content: &str) -> Result<String> {
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(path_raw) = trimmed
            .strip_prefix("<!-- INCLUDE:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            let path = format!("{AGENTS_DIR}/{}", path_raw.trim());
            let included = std::fs::read_to_string(&path)
                .with_context(|| format!("INCLUDE: failed to read '{path}'"))?;
            out.push_str(&format!("<included_file path=\"{path}\">\n"));
            out.push_str(&resolve_includes(&included)?);
            out.push_str("</included_file>\n");
        } else if trimmed == "<!-- AGENTS_LIST -->" {
            out.push_str(&render_agents_list()?);
        } else if trimmed == "<!-- MCP_LIST -->" {
            // Replaced at request time in build_openai_messages with dynamic
            // active/hidden sections. Leave a sentinel so the injection point
            // is preserved and positioned correctly in the prompt.
            out.push_str("__MCP_LIST__\n");
        } else if let Some(key) = trimmed
            .strip_prefix("<!-- ")
            .and_then(|s| s.strip_suffix(" -->"))
            .filter(|k| k.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        {
            // Generic runtime substitution: <!-- KEY --> → __KEY__ sentinel.
            // Replaced at request time via SendMessageOptions::system_substitutions.
            out.push_str(&format!("__{key}__\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

fn render_agents_list() -> Result<String> {
    let agents = discover()?;
    let mut out = String::new();
    for agent in agents.iter().filter(|a| a.id != "main" && !a.is_system_agent) {
        out.push_str(&format!("- **{}** — {}\n", agent.id, agent.description));
    }
    Ok(out)
}
