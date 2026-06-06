use std::sync::Arc;

use serde_json::{Value, json};

use crate::chat_hub::ChatHub;
use crate::session::handler::{InterfaceTool, ToolFuture};

/// Build a `notify` InterfaceTool bound to the given `ChatHub`.
///
/// `label` is the source attribution prepended to the briefing, e.g. `"TIC"`.
/// The main agent sees: `"{label} sent the following briefing: {briefing}"`.
pub fn make_tool(hub: Arc<ChatHub>, label: impl Into<String>) -> InterfaceTool {
    let label = label.into();
    let definition = json!({
        "type": "function",
        "function": {
            "name": crate::tools::tool_names::NOTIFY,
            "description": "Send a notification briefing to the user's active home conversation. \
                             The briefing is injected as if the assistant spontaneously raised its hand. \
                             Call at most once per background tick. Keep the briefing concise (2–4 sentences), \
                             contextualised, and actionable.",
            "parameters": {
                "type": "object",
                "properties": {
                    "briefing": {
                        "type": "string",
                        "description": "The message to surface to the user. Written in first person, \
                                        as the assistant speaking directly. No markdown, no lists — \
                                        plain prose only."
                    }
                },
                "required": ["briefing"]
            }
        }
    });

    let handler = Arc::new(move |args: Value| -> ToolFuture {
        let hub   = Arc::clone(&hub);
        let label = label.clone();
        Box::pin(async move {
            let briefing = args["briefing"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("notify: missing required parameter 'briefing'"))?
                .to_string();
            let decorated = format!("{label} sent the following briefing: {briefing}");
            hub.notify(decorated).await?;
            Ok("Notification queued.".to_string())
        })
    });

    InterfaceTool { definition, handler }
}
