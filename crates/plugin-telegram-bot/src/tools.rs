use std::sync::Arc;

use serde_json::json;
use teloxide::prelude::*;
use teloxide::types::InputFile;

use core_api::interface_tool::InterfaceTool;
use core_api::tts::{TextToSpeech, TtsProvider};

/// Returns all LLM-callable tools available in a Telegram session.
///
/// Each tool captures `bot` and `chat_id` so its handler can send content
/// back to the user without any additional context.
///
/// `send_voice_message` is included only when at least one TTS provider is active.
///
/// # Adding a new tool
/// Implement a private `fn <name>_tool(bot: Bot, chat_id: ChatId, ...) -> InterfaceTool`
/// and push it into the vec returned by this function.
pub(crate) async fn interface_tools(
    bot:     Bot,
    chat_id: ChatId,
    tts:     &dyn TtsProvider,
) -> Vec<InterfaceTool> {
    let mut tools = vec![send_attachment_tool(bot.clone(), chat_id)];

    if let Some(synth) = tts.get().await {
        tools.push(send_voice_tool(bot, chat_id, synth));
    }

    tools
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

// ── send_voice_message ────────────────────────────────────────────────────────

fn send_voice_tool(bot: Bot, chat_id: ChatId, synth: Arc<dyn TextToSpeech>) -> InterfaceTool {
    let instructions_hint = synth
        .instructions()
        .map(|i| format!("\n\nVoice instructions: {i}"))
        .unwrap_or_default();

    InterfaceTool {
        definition: json!({
            "type": "function",
            "function": {
                "name": "send_voice_message",
                "description": format!(
                    "Synthesise text to speech and send it to the user as a Telegram voice message. \
                     Use when audio is a better medium than text — e.g. short answers, \
                     confirmations, or when the user asks you to speak.{instructions_hint}"
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type":        "string",
                            "description": "The text to synthesise and send as audio."
                        }
                    },
                    "required": ["text"]
                }
            }
        }),
        handler: Arc::new(move |args| {
            let bot   = bot.clone();
            let synth = Arc::clone(&synth);
            Box::pin(async move {
                let text = args["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("send_voice_message: missing `text`"))?;

                let audio = synth
                    .synthesize(text, None)
                    .await
                    .map_err(|e| anyhow::anyhow!("send_voice_message: TTS error: {e}"))?;

                bot.send_voice(chat_id, InputFile::memory(audio))
                    .await
                    .map_err(|e| anyhow::anyhow!("send_voice_message: Telegram error: {e}"))?;

                Ok("Voice message sent.".to_string())
            })
        }),
    }
}
