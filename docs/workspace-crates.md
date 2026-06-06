# Workspace Crates

Independent library crates in `crates/`. None depend on the main `skald` binary crate.

---

## `core-api` — `crates/core-api/`

Shared contract types and traits used by both the main crate and future independent plugin crates.

### Modules

| Module | Contents |
| --- | --- |
| `core_api::chatbot` | `ChatbotClient` trait, `Message`, `Role`, `ChatOptions`, `ChatResponse`, `LlmTurn`, `ToolCall`, `LlmRawMeta` |
| `core_api::provider` | `ApiProvider` trait, `ApiProviderRegistry` trait, `ProviderUiMeta`, `ProviderField`, `ServiceType`, `BuiltLlmClient`; DB record types: `LlmProviderRecord`, `LlmModelRecord`, `LlmStrength`, `RemoteLlmModelInfo` |
| `core_api::tts` | `TextToSpeech` trait, `TtsProvider`, `TtsRegistry`; `TtsModelRecord`, `RemoteTtsModelInfo` |
| `core_api::transcribe` | `Transcribe` trait, `TranscribeProvider`, `TranscribeRegistry`; `TranscribeModelRecord`, `RemoteTranscribeModelInfo` |
| `core_api::image_generate` | `ImageGenerate` trait, `ImageGenerateRegistry`; `ImageGenerateModelRecord` |
| `core_api::events` | `ServerEvent`, `GlobalEvent`, `ClientMessage`, `InboundDataMessage` |
| `core_api::bus` | `ChatEventBus` — in-process broadcast for completed turns |
| `core_api::interface_tool` | `InterfaceTool`, `ToolFuture` — LLM-callable tools injected by interfaces |
| `core_api::chat_hub` | `SendMessageOptions`, `ChatHubApi` trait |
| `core_api::location` | `GpsCoord`, `LocationEntry`, `LocationManager`; `LocationUpdater` trait |
| `core_api::tool` | `Tool` trait, `ToolCategory`, `ToolDescriptionLength`, `truncate_label` |
| `core_api::memory` | `Memory` trait — pluggable long-term memory backend contract |
| `core_api::remote` | `RemoteAccess` trait — mesh/remote-connectivity provider contract |
| `core_api::plugin` | `Plugin` trait, `PluginContext`, `RouterFactory` — plugin lifecycle contract and dependency bag |

### `ChatHubApi` trait

Defines the surface a plugin needs to interact with the agent system:

```rust
#[async_trait]
pub trait ChatHubApi: Send + Sync {
    async fn register(&self, source_id: &str);
    async fn send_message(&self, source_id: &str, prompt: &str, opts: SendMessageOptions) -> anyhow::Result<()>;
    async fn clear(&self, source_id: &str) -> anyhow::Result<i64>;
    fn events(&self, source_id: &str) -> broadcast::Receiver<GlobalEvent>;
    async fn set_home(&self, source_id: &str) -> anyhow::Result<()>;
    async fn context_info(&self, source_id: &str) -> anyhow::Result<(Option<i64>, Option<i64>)>;
    async fn force_compact(&self, source_id: &str) -> anyhow::Result<bool>;
    async fn resume(&self, source_id: &str) -> anyhow::Result<()>;
    async fn approve(&self, request_id: i64);
    async fn reject(&self, request_id: i64, note: String);
    async fn resolve_question(&self, source_id: &str, request_id: i64, answer: String);
}
```

`ChatHub` in `src/core/chat_hub/mod.rs` implements this trait. To call trait methods on `Arc<ChatHub>`, import the trait: `use crate::chat_hub::ChatHubApi as _;`.

### `InterfaceTool`

```rust
pub struct InterfaceTool {
    pub definition: Value,   // OpenAI tool definition
    pub handler: Arc<dyn Fn(Value) -> ToolFuture + Send + Sync>,
}
```

Interface tools are injected per-turn via `SendMessageOptions::interface_tools`. They are only visible to the root agent — sub-agents do not inherit them (except `show_mcp_tools` which is re-injected explicitly).

---

## Plugin Extraction Roadmap

The goal is to allow plugins to live in their own workspace crates without depending on the full main binary. All plugins depend only on `core-api` and external crates.

### Extracted plugins

| Plugin | Crate | Doc |
| --- | --- | --- |
| `honcho` | `crates/plugin-honcho/` | [honcho.md](honcho.md) |
| `remote_connectivity` | `crates/plugin-tailscale-remote/` | [remote.md](remote.md) |
| `whisper_local` | `crates/plugin-transcribe-whisper-local/` | [whisper-local.md](whisper-local.md) |
| `telegram` | `crates/plugin-telegram-bot/` | [telegram.md](telegram.md) |
| `orpheus_tts_3b` | `crates/plugin-tts-orpheus-3b/` | [tts-providers.md](tts-providers.md) |
| `kokoro_tts` | `crates/plugin-tts-kokoro/` | [tts-providers.md](tts-providers.md) |
| `elevenlabs` | `crates/plugin-elevenlabs/` | [tts-providers.md](tts-providers.md) |

### Remaining in main crate

All plugins have been extracted to independent workspace crates. ElevenLabs (TTS + transcription) was extracted into `crates/plugin-elevenlabs/` — it registers itself as an `ApiProvider` so the existing `llm_providers` + `tts_models` / `transcribe_models` UI continues to work unchanged.

### All `core-api` contracts needed by plugins

| Dependency | Status |
| --- | --- |
| `core_api::chatbot::ChatbotClient` (+ associated types) | ✅ In `core-api` |
| `core_api::provider::{ApiProvider, ApiProviderRegistry, LlmProviderRecord, …}` | ✅ In `core-api` |
| `core_api::tts::{TextToSpeech, TtsProvider, TtsRegistry, TtsModelRecord, …}` | ✅ In `core-api` |
| `core_api::transcribe::{Transcribe, TranscribeProvider, TranscribeRegistry, TranscribeModelRecord, …}` | ✅ In `core-api` |
| `core_api::image_generate::{ImageGenerate, ImageGenerateRegistry, ImageGenerateModelRecord}` | ✅ In `core-api` |
| `core_api::events::{ServerEvent, GlobalEvent}` | ✅ In `core-api` |
| `core_api::interface_tool::InterfaceTool` | ✅ In `core-api` |
| `core_api::chat_hub::{ChatHubApi, SendMessageOptions}` | ✅ In `core-api` |
| `core_api::location::{GpsCoord, LocationManager, LocationUpdater}` | ✅ In `core-api` |
| `core_api::remote::RemoteAccess` | ✅ In `core-api` |
| `core_api::plugin::{Plugin, PluginContext, RouterFactory}` | ✅ In `core-api` |
| `core_api::bus::{BusEvent, ChatEvent, ChatEventRole, RecvError}` | ✅ In `core-api` |
| `core_api::memory::Memory` | ✅ In `core-api` |
| `core_api::tool::{Tool, ToolCategory}` | ✅ In `core-api` |

---

## Decoupling Pattern — OnceLock extraction

When a plugin cannot receive its typed deps at construction time (because `Skald` is built after plugin registration), use `std::sync::OnceLock` to extract and name the deps on first `start()`:

```rust
pub struct MyPlugin {
    // named, typed deps — no Arc<Skald>
    chat_hub:    OnceLock<Arc<dyn ChatHubApi>>,
    some_config: OnceLock<u16>,
}

fn extract_deps(&self, ctx: &PluginContext) {
    let _ = self.chat_hub.set(Arc::clone(&ctx.chat_hub));
    let _ = self.some_config.set(ctx.web_port);
}

async fn start(&self, ctx: PluginContext) -> Result<()> {
    self.extract_deps(&ctx);
    self.do_start().await  // no Skald needed here
}
```

`OnceLock::set` is idempotent — safe across multiple `reload()` calls. The values must be stable for the process lifetime (config values, `Arc` handles to singletons).

`RemotePlugin` (`crates/plugin-tailscale-remote/src/lib.rs`) uses this pattern with three deps: `port`, `remote_slot`, and `router_factory` — all sourced from `PluginContext`.

---

## `llm-client` — `crates/llm-client/`

Concrete LLM provider implementations: OpenAI-compatible, native Anthropic, Ollama, LmStudio.

Depends on `core-api` — `ChatbotClient` and all associated types (`Message`, `Role`, `ChatOptions`, `ChatResponse`, `LlmTurn`, `ToolCall`, `LlmRawMeta`) are defined there and re-exported from `llm-client` for backward compatibility.

Utility functions that depend on `reqwest` (`headers_to_json`, `redact_key`) remain in `llm-client` and are not part of `core-api`.

---

## `mcp-client` — `crates/mcp-client/`

MCP (Model Context Protocol) client over stdio and SSE transports. Used by `McpManager`.

---

## `honcho-client` — `crates/honcho-client/`

HTTP client for the Honcho long-term memory service. Used by `crates/plugin-honcho/`.

---

## `plugin-honcho` — `crates/plugin-honcho/`

Independent plugin crate for the Honcho long-term memory integration. Depends only on `core-api` and `honcho-client`. See [honcho.md](honcho.md).

---

## `plugin-tailscale-remote` — `crates/plugin-tailscale-remote/`

Independent plugin crate that exposes the web app on a Tailscale mesh network. Depends only on `core-api` and external crates (`tailscale`, `axum`, `tokio`, …). See [remote.md](remote.md).

Contains three modules:

| Module | Contents |
| --- | --- |
| `lib.rs` | `RemotePlugin` — plugin lifecycle, provider selection |
| `tailscale_sys.rs` | `TailscaleSystemProvider` — reads IP from system `tailscaled` daemon |
| `tailscale.rs` | `TailscaleEmbeddedProvider` — embedded netstack via `tailscale-rs` (feature-gated) |

Feature flags (in `crates/plugin-tailscale-remote/Cargo.toml`):

```toml
[features]
default = ["remote-tailscale"]
remote-tailscale = ["dep:tailscale"]
```

---

## `plugin-transcribe-whisper-local` — `crates/plugin-transcribe-whisper-local/`

Independent plugin crate providing local Speech-to-Text via whisper.cpp (Metal-accelerated on Apple Silicon). Depends only on `core-api`, `whisper-rs`, and `hound`. See [whisper-local.md](whisper-local.md).

`whisper-rs` and `hound` live exclusively in this crate — the main binary no longer depends on them directly.

### Key types

| Type | Role |
| --- | --- |
| `WhisperLocalPlugin` | `Plugin` impl — manages model lifecycle and registers/deregisters `WhisperLocalTranscriber` |
| `WhisperLocalTranscriber` | `Transcribe` impl — lightweight handle passed to `TranscribeManager` at `start()` |

Audio is converted to 16 kHz mono WAV via `ffmpeg` before being fed to whisper.cpp. Model must be a GGML `.bin` file; path is configured via the plugins REST API.

---

## `plugin-telegram-bot` — `crates/plugin-telegram-bot/`

Independent plugin crate for the private Telegram bot interface. Depends only on `core-api`, `teloxide`, and supporting crates (`tokio-util`, `chrono`, `rand`, `regex`). See [telegram.md](telegram.md).

`teloxide` and `tokio-util` live exclusively in this crate — the main binary no longer depends on them directly. The name `plugin-telegram-bot` distinguishes a bot-account integration from a potential future userbot (personal account) plugin.

### Source modules

| Module | Contents |
| --- | --- |
| `lib.rs` | `TelegramPlugin` — plugin lifecycle, bot startup, dispatcher wiring; `TgShared` holds `Arc<dyn TtsProvider>` |
| `events.rs` | `persistent_forwarder` — subscribes to ChatHub events and forwards to Telegram; `callback_handler` — inline keyboard button presses |
| `handlers.rs` | `message_handler`, `edited_message_handler` — incoming message classification and dispatch |
| `auth.rs` | `WhitelistFile`, pairing flow, `whitelist_watchdog` |
| `attachments.rs` | `TelegramAttachment` — download and describe documents, photos, locations |
| `helpers.rs` | `escape_html`, `label_to_html`, `send_long`, Markdown→HTML sanitizer |
| `tools.rs` | `interface_tools` (async) — `send_attachment` always present; `send_voice_message` injected only when at least one TTS provider is active |

`send_voice_message` calls `TtsProvider::get()` at message time, synthesises text via the highest-priority active provider, and sends the result with `bot.send_voice()`. The tool's description automatically includes the provider's `instructions()` field so the LLM knows how to format text for that specific voice engine.
