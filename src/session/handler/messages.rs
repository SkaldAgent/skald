use std::collections::HashSet;

use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::compactor::SUMMARY_PREFIX;
use crate::db::{chat_history, chat_llm_tools, chat_summaries};

use super::ChatSessionHandler;

impl ChatSessionHandler {
    /// Builds a raw OpenAI-format message array from the persisted history,
    /// reconstructing assistant tool-call entries and tool-result entries from
    /// the `chat_llm_tools` table.
    ///
    /// `active_mcp_grants` is the set of MCP server names currently granted for
    /// this session. It is used to build the compact MCP availability list that is
    /// injected into the system prompt so the LLM knows which servers it can activate.
    /// Builds the OpenAI-format message array for one LLM round.
    ///
    /// ## Message order (optimised for prefix KV caching)
    ///
    /// ```text
    /// 1. [system]  Static content — AGENT.md + memory files + extra_system_static + MCP list
    ///              Tagged cache_control:ephemeral when cache_hints=true (Anthropic via OpenRouter).
    ///              For all other providers this is a plain string that never changes turn-to-turn,
    ///              so the provider's own automatic prefix cache hits on it.
    ///
    /// 2. [system]  Scratchpad — emitted only when non-empty, BEFORE the conversation.
    ///              Separate from the static block so a scratchpad write mid-turn only
    ///              invalidates this small message, not the large static prefix.
    ///
    /// 3. [system]  Compaction summary — if a summary exists for this stack.
    ///
    /// 4. [user / assistant / tool]  Conversation history.
    ///
    /// 5. [system]  Dynamic tail — extra_system_dynamic (e.g. Honcho long-term memories)
    ///              + current date/time/OS/cwd.  Placed AFTER the conversation so it
    ///              does not disturb the stable prefix; also recency-biased attention
    ///              ensures the model reads fresh context right before generating.
    ///
    /// 6. [system]  Tail reminder — short anti-drift reminder (e.g. Telegram format).
    /// ```
    pub(super) async fn build_openai_messages(
        &self,
        pool:                 &SqlitePool,
        stack_id:             i64,
        agent_id:             &str,
        extra_system_static:  Option<&str>,
        extra_system_dynamic: Option<&str>,
        tail_reminder:        Option<&str>,
        active_mcp_grants:    &HashSet<String>,
        cache_hints:          bool,
    ) -> anyhow::Result<Vec<Value>> {

        // ── 1. Static system message ──────────────────────────────────────────
        // Contents: AGENT.md + inject_memory files + extra_system_static + MCP list.
        // Nothing here should change turn-to-turn; it is the KV-cache prefix.

        let mut static_content = crate::agents::load_prompt(agent_id)?;

        // Inject memory files declared in meta.json into the system prompt.
        let meta = crate::agents::load_meta(agent_id)?;
        if !meta.inject_memory.is_empty() {
            static_content.push_str(
                "\n\n---\nThe following memory files have been loaded automatically. \
                 You can edit them with `edit_file` or `write_file` using the path shown.\n"
            );
            for mem_path in &meta.inject_memory {
                let content = match crate::tools::fs::resolve(mem_path) {
                    Ok(abs) => tokio::fs::read_to_string(&abs).await.ok(),
                    Err(_)  => None,
                };
                match content {
                    Some(c) => static_content.push_str(&format!(
                        "\n<memory_file path=\"{mem_path}\">\n{c}\n</memory_file>\n"
                    )),
                    None => static_content.push_str(&format!(
                        "\n<memory_file path=\"{mem_path}\">\n(file non ancora creato)\n</memory_file>\n"
                    )),
                }
            }
        }

        if let Some(extra) = extra_system_static {
            static_content.push_str("\n\n---\n");
            static_content.push_str(extra);
        }

        // Replace the __MCP_LIST__ sentinel with the dynamic active/hidden breakdown.
        if static_content.contains("__MCP_LIST__") {
            static_content = static_content.replace(
                "__MCP_LIST__",
                &self.render_mcp_list(active_mcp_grants),
            );
        }

        let static_msg = if cache_hints {
            // Anthropic-compatible content array: the one block is tagged so the
            // provider caches everything up to this point as a KV prefix.
            json!({
                "role": "system",
                "content": [{ "type": "text", "text": static_content, "cache_control": { "type": "ephemeral" } }]
            })
        } else {
            json!({ "role": "system", "content": static_content })
        };

        let mut out = vec![static_msg];

        // ── 2. Scratchpad system message (before conversation) ────────────────
        // Emitted only when non-empty.  Kept separate from the static block so
        // that a mid-turn scratchpad update only invalidates this small message.
        let scratch = crate::db::scratchpad::for_session(pool, self.session_id).await?;
        if !scratch.is_empty() {
            let mut s = String::from(
                "<scratchpad>\n  \
                 <!-- Temporary notes shared by all agents in this session. Not persisted across sessions. -->\n"
            );
            for (k, v) in &scratch {
                s.push_str(&format!("  <note key=\"{k}\">{v}</note>\n"));
            }
            s.push_str("</scratchpad>");
            out.push(json!({ "role": "system", "content": s }));
        }

        // ── Context compaction: inject summary + load only messages after boundary ──
        // If a compaction summary exists for this stack, inject it immediately
        // after the system prompt and load only the raw messages that follow the
        // summary boundary.  Otherwise fall back to loading the full history.
        let summary = chat_summaries::latest_for_stack(pool, stack_id).await?;
        let mut history = match &summary {
            Some(s) => {
                // Inject the summary as a system message so it is treated as
                // authoritative context, not part of the user/assistant dialogue.
                // OpenAI-compatible providers accept mid-conversation system
                // messages natively.  AnthropicClient collects ALL system
                // messages and merges them into the single `system:` parameter,
                // so this works transparently for Anthropic as well.
                //
                // SUMMARY_PREFIX (from compactor.rs) is the Hermes-style handoff
                // header that tells the LLM this is reference material, not live
                // instructions, and to resume from "## Active Task".
                out.push(json!({
                    "role": "system",
                    "content": format!(
                        "{SUMMARY_PREFIX}\n\n{}\n\n\
                         [End of context summary — the following messages are the most recent exchanges in full.]",
                        s.content
                    )
                }));
                chat_history::for_stack_since(pool, stack_id, s.covers_up_to_message_id).await?
            }
            None => chat_history::for_stack(pool, stack_id).await?,
        };

        // Safety floor: apply max_history_messages only when compaction is
        // disabled.  When compaction is configured it owns the token budget;
        // silently truncating by count would discard history that the compactor
        // should summarise instead — and without leaving any trace.
        if self.compactor.is_none() && history.len() > self.max_history_messages {
            history.drain(..history.len() - self.max_history_messages);
            // Guarantee the remaining history starts with a user/agent message.
            // A drain that cuts in the middle of a user+assistant pair would
            // leave an orphaned assistant message at the head, breaking strict
            // user→assistant alternation required by some providers (OpenRouter).
            if matches!(history.first().map(|m| &m.role), Some(chat_history::Role::Assistant)) {
                history.drain(..1);
            }
        }

        // Find the boundary for tool-result hiding: the index of the last user/agent
        // message in the (possibly truncated) history.  Tool results that belong to
        // assistant messages *before* this index come from previous turns and are
        // eligible for replacement when they exceed `max_tool_result_chars`.
        // Tool results from the current turn (at or after the boundary) are always
        // shown in full so the LLM can work with them.
        let current_turn_boundary = history
            .iter()
            .rposition(|e| matches!(e.role, chat_history::Role::User | chat_history::Role::Agent));

        for (idx, entry) in history.iter().enumerate() {
            let is_previous_turn = current_turn_boundary.map_or(false, |b| idx < b);

            match entry.role {
                chat_history::Role::User | chat_history::Role::Agent => {
                    out.push(json!({ "role": "user", "content": entry.content }));
                }
                chat_history::Role::Assistant => {
                    let tool_calls = chat_llm_tools::for_message(pool, entry.id).await?;

                    if tool_calls.is_empty() {
                        let mut msg = json!({ "role": "assistant", "content": entry.content });
                        if let Some(rc) = &entry.reasoning_content {
                            msg["reasoning_content"] = rc.clone().into();
                        }
                        out.push(msg);
                    } else {
                        // Reconstruct the assistant message that requested tool calls.
                        let tc_array: Vec<Value> = tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id":   format!("tc_{}", tc.id),
                                "type": "function",
                                "function": {
                                    "name":      tc.name,
                                    "arguments": tc.arguments.as_deref().unwrap_or("{}"),
                                }
                            }))
                            .collect();

                        let mut msg = json!({
                            "role":       "assistant",
                            "content":    entry.content,
                            "tool_calls": tc_array,
                        });
                        if let Some(rc) = &entry.reasoning_content {
                            msg["reasoning_content"] = rc.clone().into();
                        }
                        out.push(msg);

                        // Then one tool-result message per call.
                        for tc in &tool_calls {
                            let result_content = match tc.status.as_str() {
                                "done"   => tc.result.as_deref().unwrap_or("").to_string(),
                                "failed" => format!(
                                    "Error: {}",
                                    tc.result.as_deref().unwrap_or("unknown error")
                                ),
                                // `pending` means the previous session was interrupted before
                                // the user approved/rejected. Treat it as failed so the LLM
                                // knows it must retry the operation.
                                _ => "Error: tool call was interrupted (connection lost before user approval). Please retry the operation.".to_string(),
                            };

                            // Replace oversized results from previous turns with an
                            // informative 1-line summary (Hermes-style).
                            // The full content is always kept in the DB.
                            let result_content = self.maybe_hide_tool_result(
                                result_content,
                                is_previous_turn,
                                &tc.name,
                                tc.arguments.as_deref(),
                            );

                            out.push(json!({
                                "role":         "tool",
                                "tool_call_id": format!("tc_{}", tc.id),
                                "content":      result_content,
                            }));
                        }
                    }
                }
            }
        }

        // ── 5. Dynamic tail system message (after conversation) ──────────────
        // Contains: Honcho long-term memories (extra_system_dynamic) + current
        // date/time/OS/cwd.  Placed at the tail so the stable prefix above is
        // never invalidated by per-turn changes, while the model still sees
        // fresh user context with recency-biased attention right before
        // generating its response.
        {
            let datetime_line = if self.datetime_config.enabled {
                let now_utc = chrono::Utc::now();
                let secs = now_utc.timestamp();

                // Apply optional rounding before formatting.
                let secs = match self.datetime_config.round_minutes {
                    Some(m) if m > 0 => {
                        let bucket = (m as i64) * 60;
                        (secs / bucket) * bucket
                    }
                    _ => secs,
                };

                // Format in the configured timezone (falls back to local).
                let formatted = match self.datetime_config.timezone.as_deref()
                    .and_then(|s| s.parse::<chrono_tz::Tz>().ok())
                {
                    Some(tz) => {
                        use chrono::TimeZone as _;
                        tz.timestamp_opt(secs, 0)
                            .single()
                            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
                            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string())
                    }
                    None => {
                        chrono::DateTime::from_timestamp(secs, 0)
                            .map(|utc| utc.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S%:z").to_string())
                            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string())
                    }
                };

                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "(unknown)".to_string());
                let os = std::env::consts::OS;
                Some(format!("Current date and time: {formatted}\nOperating system: {os}\nWorking directory: {cwd}"))
            } else {
                None
            };

            let tail = match (extra_system_dynamic, datetime_line.as_deref()) {
                (Some(dyn_ctx), Some(dt)) => Some(format!("{dyn_ctx}\n\n---\n{dt}")),
                (Some(dyn_ctx), None)     => Some(dyn_ctx.to_string()),
                (None,          Some(dt)) => Some(dt.to_string()),
                (None,          None)     => None,
            };
            if let Some(content) = tail {
                out.push(json!({ "role": "system", "content": content }));
            }
        }

        // ── 6. Tail reminder ──────────────────────────────────────────────────
        // Short anti-drift reminder injected at the very end (e.g. Telegram
        // format rules), closest to the generation point.
        if let Some(reminder) = tail_reminder {
            out.push(json!({ "role": "system", "content": reminder }));
        }

        Ok(out)
    }

    /// Returns the tool result as-is, or replaces it with an informative 1-line
    /// summary (Hermes-style) when all of the following hold:
    ///   - `is_previous_turn` is true (the result belongs to a completed turn)
    ///   - `self.max_tool_result_chars` is set
    ///   - the result exceeds that limit
    ///
    /// The database content is never touched; this only affects what the LLM sees.
    fn maybe_hide_tool_result(
        &self,
        result:           String,
        is_previous_turn: bool,
        tool_name:        &str,
        arguments:        Option<&str>,
    ) -> String {
        if !is_previous_turn {
            return result;
        }
        let Some(limit) = self.max_tool_result_chars else {
            return result;
        };
        if result.len() <= limit {
            return result;
        }
        summarize_tool_result(tool_name, arguments, &result)
    }

    /// Builds the MCP list section that replaces the `__MCP_LIST__` sentinel.
    /// Groups running servers into "available" (not yet granted) and "active" (granted).
    /// Tool names are listed as hints so the LLM knows what each server offers.
    fn render_mcp_list(&self, active_mcp_grants: &std::collections::HashSet<String>) -> String {
        let all_tools = self.mcp.tools();

        // server_name → sorted tool short-names
        let mut server_tools: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for t in &all_tools {
            server_tools
                .entry(t.server_name.clone())
                .or_default()
                .push(t.name.clone());
        }
        for tools in server_tools.values_mut() {
            tools.sort();
        }

        if server_tools.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "## MCP servers\n\
             Once activated, tools are called as `mcp__<server>__<tool>` (e.g. `mcp__gmail__send_message`).\n"
        );

        let hidden: Vec<&String> = server_tools.keys()
            .filter(|n| !active_mcp_grants.contains(*n))
            .collect();
        let active: Vec<&String> = server_tools.keys()
            .filter(|n| active_mcp_grants.contains(*n))
            .collect();

        if !hidden.is_empty() {
            out.push_str("\n**Available** — call `show_mcp_tools([\"name\", ...])` to load tools:\n");
            for name in &hidden {
                let tools = server_tools[*name].join(", ");
                out.push_str(&format!("- `{name}`: {tools}\n"));
            }
        }

        if !active.is_empty() {
            out.push_str("\n**Active** — tools already loaded in context:\n");
            for name in &active {
                let tools = server_tools[*name].join(", ");
                out.push_str(&format!("- `{name}`: {tools}\n"));
            }
        }

        out
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Creates an informative 1-line summary of a tool call result.
///
/// Ported from Hermes' `_summarize_tool_result` — produces human-readable
/// descriptions like:
/// ```text
/// [execute_cmd] ran `cargo build` → exit 0, 47 lines output
/// [read_file] read src/main.rs (3,200 chars)
/// [write_file] wrote to agents/foo/AGENT.md
/// ```
///
/// Used by `maybe_hide_tool_result` to replace oversized previous-turn results
/// with compact but useful context instead of a generic "hidden" placeholder.
fn summarize_tool_result(tool_name: &str, arguments: Option<&str>, result: &str) -> String {
    let args: serde_json::Value = arguments
        .and_then(|a| serde_json::from_str(a).ok())
        .unwrap_or(serde_json::Value::Null);

    let char_count = result.len();
    let line_count = if result.trim().is_empty() { 0 } else { result.lines().count() };

    /// Extract a string field from the parsed args Value.
    fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> &'a str {
        args[key].as_str().unwrap_or("?")
    }

    match tool_name {
        "execute_cmd" => {
            let cmd = args["command"].as_str().unwrap_or("");
            let cmd_display = if cmd.len() > 80 {
                format!("{}…", &cmd[..77])
            } else {
                cmd.to_string()
            };
            // Our execute_cmd output format: "exit: N\n--- stdout ---\n…"
            let exit_code = result
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("exit: "))
                .unwrap_or("?");
            format!("[execute_cmd] ran `{cmd_display}` → exit {exit_code}, {line_count} lines output")
        }

        "read_file" | "read_file_chunk" => {
            let path = arg_str(&args, "path");
            format!("[{tool_name}] read {path} ({char_count} chars)")
        }

        "write_file" => {
            let path = arg_str(&args, "path");
            format!("[write_file] wrote to {path}")
        }

        "edit_file" | "patch_file" => {
            let path = arg_str(&args, "path");
            format!("[{tool_name}] edited {path}")
        }

        "list_dir" | "glob" => {
            let path = args["path"].as_str()
                .or_else(|| args["pattern"].as_str())
                .unwrap_or("?");
            format!("[{tool_name}] {path} ({char_count} chars)")
        }

        "list_agents" => {
            format!("[list_agents] listed agents ({char_count} chars)")
        }

        "call_agent" => {
            let agent = arg_str(&args, "agent_id");
            format!("[call_agent] → {agent} ({char_count} chars result)")
        }

        "show_mcp_tools" => {
            let servers = args["servers"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "?".to_string());
            format!("[show_mcp_tools] loaded: {servers}")
        }

        _ if tool_name.starts_with("mcp__") => {
            format!("[{tool_name}] ({char_count} chars result)")
        }

        _ => {
            // Generic fallback: first arg key=value + size
            let first_arg = args.as_object()
                .and_then(|m| m.iter().next())
                .map(|(k, v)| {
                    let sv = v.as_str().unwrap_or_default();
                    let sv = if sv.len() > 40 { &sv[..40] } else { sv };
                    format!(" {k}={sv}")
                })
                .unwrap_or_default();
            format!("[{tool_name}]{first_arg} ({char_count} chars result)")
        }
    }
}
