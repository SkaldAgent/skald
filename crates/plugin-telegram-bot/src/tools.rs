use std::sync::Arc;

use serde_json::json;
use teloxide::prelude::*;
use teloxide::types::InputFile;

use core_api::interface_tool::InterfaceTool;

/// Returns all LLM-callable tools available in a Telegram session.
///
/// Each tool captures `bot` and `chat_id` so its handler can send content
/// back to the user without any additional context.
///
/// # Adding a new tool
/// Implement a private `fn <name>_tool(bot: Bot, chat_id: ChatId) -> InterfaceTool`
/// and push it into the vec returned by this function.
pub(crate) fn interface_tools(bot: Bot, chat_id: ChatId) -> Vec<InterfaceTool> {
    vec![
        send_attachment_tool(bot, chat_id),
    ]
}

// ── send_attachment ───────────────────────────────────────────────────────────

fn send_attachment_tool(bot: Bot, chat_id: ChatId) -> InterfaceTool {
    InterfaceTool {
        definition: json!({
            "type": "function",
            "function": {
                "name": "send_attachment",
                "description": "Send a file from the local filesystem to the user on Telegram.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type":        "string",
                            "description": "Absolute or relative path to the file to send."
                        },
                        "caption": {
                            "type":        "string",
                            "description": "Optional caption shown below the file."
                        }
                    },
                    "required": ["file_path"]
                }
            }
        }),
        handler: Arc::new(move |args| {
            let bot     = bot.clone();
            Box::pin(async move {
                let file_path = args["file_path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("send_attachment: missing `file_path`"))?;
                let caption = args["caption"].as_str().map(str::to_string);

                let path = std::path::Path::new(file_path);
                if !path.exists() {
                    anyhow::bail!("send_attachment: file not found: {file_path}");
                }

                let mut req = bot.send_document(chat_id, InputFile::file(path));
                if let Some(cap) = caption {
                    req = req.caption(cap);
                }

                req.await.map_err(|e| anyhow::anyhow!("send_attachment: Telegram error: {e}"))?;
                Ok(format!("File sent: {file_path}"))
            })
        }),
    }
}
