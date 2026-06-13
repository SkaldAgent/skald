# Plugin System

Plugins are long-running subsystems compiled into the binary and managed by `PluginManager`. They receive a `PluginContext` (a bundle of `Arc<dyn Trait>` deps) and run independently from the Axum web server.

---

## Plugin Trait

```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    fn id(&self)          -> &str;
    fn name(&self)        -> &str;
    fn description(&self) -> &str;
    fn is_running(&self)  -> bool;
    async fn start(&self, ctx: PluginContext)  -> Result<()>;
    async fn reload(&self, enabled: bool, config: Value, ctx: PluginContext) -> Result<()>;
    async fn stop(&self)  -> Result<()>;
    fn as_any(&self)      -> &dyn Any;
}
```

`start()` and `reload()` must **spawn internal tasks and return immediately**.
`stop()` must cancel all internal tasks and **await their completion**.

`PluginContext` (`crates/core-api/src/plugin.rs`) carries typed `Arc<dyn Trait>` deps sourced from `Skald` — plugins use only what they need.

---

## PluginManager — `src/core/plugin/mod.rs`

| Method | Purpose |
|---|---|
| `register(plugin)` | Add a plugin before startup (build phase) |
| `set_skald(Arc<Skald>)` | Wire core after `Skald` is built |
| `set_router_factory(factory)` | Provide Axum router factory (called by `WebFrontend`) |
| `set_web_port(port)` | Provide HTTP port for plugins that need it (called by `WebFrontend`) |
| `start_enabled()` | Start all DB-enabled plugins |
| `stop_all()` | Graceful shutdown on SIGINT |
| `toggle(id, enabled)` | Enable/disable and start/stop at runtime |
| `list()` | `Vec<PluginInfo>` with enabled + running flags |

### Startup sequence

```
main.rs:
  plugins = vec![Arc::new(HonchoPlugin::new()), Arc::new(TelegramPlugin::new(...)), …]

Skald::new(core_cfg, plugins):
  PluginManager::new(pool) → register_arc(plugin) for each plugin → Arc::new(plugin_manager)
  → build tool_registry (list_items / toggle_item reference Arc<PluginManager>)
  → Arc::new(Skald { … })
  → plugin_manager.set_skald(Arc::clone(&skald))

WebFrontend::start():
  → plugin_manager.set_router_factory(factory)
  → plugin_manager.set_web_port(port)
  → plugin_manager.start_enabled().await
```

### Enabled/disabled persistence

Plugin state and configuration are stored exclusively in the `plugins` SQLite table (`id TEXT PK, enabled INTEGER, config TEXT`). `config.yml` has no plugin section — plugins are configured at runtime via the REST API (`PUT /api/plugins/{id}`) or by asking the agent to use `toggle_item` (kind=plugin).

---

## Adding a New Plugin

Plugins live in independent workspace crates (see [workspace-crates.md](workspace-crates.md)):

1. Create `crates/plugin-<name>/` with a `Cargo.toml` depending on `core-api` and any needed external crates.
2. Implement `core_api::plugin::Plugin` in `crates/plugin-<name>/src/lib.rs`.
3. Add the crate to the workspace `members` list and as a dependency in the main `Cargo.toml`.
4. In `src/main.rs`, add `Arc::new(plugin_name::MyPlugin::new(...))` to the `plugins` vec before `Skald::new()`.
5. Rebuild — no restart needed for toggle.

---

## LLM Tools

| Tool | Description |
|---|---|
| `list_items` (type=plugins) | All plugins with enabled + running status |
| `toggle_item` (kind=plugin) | Enable/disable a plugin by id (immediate + persisted) |
| `configure_plugin` | Update a plugin's config JSON and restart it immediately |

---

## Available Plugins

| Plugin id | Source | Doc |
|---|---|---|
| `honcho` | `crates/plugin-honcho/src/lib.rs` | [honcho.md](honcho.md) |
| `remote_connectivity` | `crates/plugin-tailscale-remote/src/lib.rs` | [remote.md](remote.md) |
| `telegram` | `crates/plugin-telegram-bot/src/lib.rs` | [telegram.md](telegram.md) |
| `whisper_local` | `crates/plugin-transcribe-whisper-local/src/lib.rs` | [whisper-local.md](whisper-local.md) |
| `comfyui` | `crates/plugin-comfyui/src/lib.rs` | [image-generate.md](image-generate.md) |

---

## Transcribe Providers and TranscribeManager

Speech-to-Text is decoupled from the plugin system via `TranscribeManager` (`src/core/transcribe/mod.rs`).

Plugins that provide transcription (e.g. `whisper_local`) register an `Arc<dyn Transcribe>` in `skald.transcribe_manager` at `start()` and deregister at `stop()`. Non-plugin providers (e.g. a future OpenRouter client) can register directly at startup without needing a full plugin lifecycle.

```rust
// trait — src/core/transcribe/mod.rs
pub trait Transcribe: Send + Sync {
    fn id(&self) -> &str;
    async fn transcribe(&self, audio: Vec<u8>, format: &str) -> Result<String>;
}
```

| Method | Purpose |
|---|---|
| `register(Arc<dyn Transcribe>)` | Add/replace a provider by id |
| `unregister(id)` | Remove a provider |
| `get()` | Returns the first available provider |

Selection strategy is currently **first registered**. Callers (e.g. Telegram) ask `skald.transcribe_manager.get().await` — they never reference a concrete type.

See [whisper-local.md](whisper-local.md) for the only current provider.

---

## Image Generators and ImageGeneratorManager

Image generation is decoupled from the plugin system via `ImageGeneratorManager` (`src/core/image_generate/`) and two traits in `core-api::image_generate` — same split as `TranscribeProvider` / `TranscribeRegistry`.

Two kinds of providers coexist:

| Kind | Source | Example |
| --- | --- | --- |
| **DB-backed** | `image_generate_models` table, built from `llm_providers` credentials | OpenRouter `x-ai/grok-2-vision` |
| **Plugin-registered** | Ephemeral — registered at runtime by plugins | ComfyUI workflows |

Plugins register via `ctx.image_generate_registry` (type `Arc<dyn ImageGenerateRegistry>`) in `PluginContext`. No dependency on the main crate is needed — only `core-api`.

```rust
// crates/core-api/src/image_generate.rs
pub trait ImageGenerate: Send + Sync { fn id(&self) -> &str; fn name(&self) -> &str; async fn generate(&self, prompt: &str) -> Result<Vec<u8>>; }
pub trait ImageGenerateRegistry: Send + Sync { async fn register(&self, provider: Arc<dyn ImageGenerate>); async fn unregister(&self, id: &str); }
```

| Method | Purpose |
|---|---|
| `ctx.image_generate_registry.register(...)` | Add a plugin provider (ephemeral) |
| `ctx.image_generate_registry.unregister(id)` | Remove a plugin provider |
| `add_model / update_model / delete_model` | DB-backed CRUD (called by REST API) |
| `list()` | Returns all active providers (plugin + DB) — used by LLM tool |
| `get(id)` | Returns a specific provider by id |
| `generate(provider_id, prompt)` | Generates and saves image, returns `(PathBuf, url)` |

The LLM interacts with providers via two tools: `image_generate_providers_list` and `image_generate`. See [image-generate.md](image-generate.md) for the full flow.

---

## TTS and TtsManager

Text-to-speech follows the same split pattern as transcribe and image_generate. `TtsManager` (`src/core/tts/`) manages both DB-backed and plugin-registered providers. Traits live in `core-api::tts`.

| Kind | Source | Example |
| --- | --- | --- |
| **DB-backed** | `tts_models` table, built from `llm_providers` credentials | OpenAI `tts-1-hd` |
| **Plugin-registered** | Ephemeral — registered at runtime by plugins | `OrpheusTtsPlugin` |

Plugins register via `ctx.tts_registry` (type `Arc<dyn TtsRegistry>`) in `PluginContext`.

```rust
// crates/core-api/src/tts.rs
pub trait TextToSpeech: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn instructions(&self) -> Option<&str>;  // default voice style
    async fn synthesize(&self, text: &str, instructions: Option<&str>) -> Result<Vec<u8>>;
}
pub trait TtsRegistry: Send + Sync {
    async fn register(&self, provider: Arc<dyn TextToSpeech>);
    async fn unregister(&self, id: &str);
}
```

| Method | Purpose |
|---|---|
| `ctx.tts_registry.register(...)` | Add a plugin TTS provider (ephemeral) |
| `ctx.tts_registry.unregister(id)` | Remove a plugin provider |

See [tts-providers.md](tts-providers.md) for the full manager API and DB schema.

---

## ApiProvider and ApiProviderRegistry

Plugins that supply an `ApiProvider` (e.g. a cloud TTS + Transcribe integration without subprocess) register via `ctx.api_provider_registry`:

```rust
// crates/core-api/src/provider.rs
#[async_trait]
pub trait ApiProvider: Send + Sync {
    fn type_id(&self)        -> &'static str;
    fn display_name(&self)   -> &'static str;
    fn supported_types(&self)-> &'static [ServiceType];
    async fn list_tts_models(&self, record: &LlmProviderRecord) -> Result<Option<Vec<RemoteTtsModelInfo>>>;
    async fn list_transcribe_models(&self, ..) -> ..;
    async fn list_llm_models(&self, ..)        -> ..;
    fn build_tts(&self, ..)             -> Option<Result<Arc<dyn TextToSpeech>>>;
    fn build_transcriber(&self, ..)     -> Option<Result<Arc<dyn Transcribe>>>;
    fn build_llm(&self, ..)             -> Option<Result<BuiltLlmClient>>;
    fn build_image_generator(&self, ..) -> Option<Result<Arc<dyn ImageGenerate>>>;
    fn ui_meta(&self) -> ProviderUiMeta;
}

pub trait ApiProviderRegistry: Send + Sync {
    fn register_plugin(&self, provider: Arc<dyn ApiProvider>);
    fn unregister_plugin(&self, type_id: &str);
}
```

`ProviderRegistry` in `src/core/provider/mod.rs` implements `ApiProviderRegistry`. It is exposed as `ctx.api_provider_registry` in `PluginContext`.

Plugin-registered providers shadow builtin ones: `ProviderRegistry::get(type_id)` checks the plugin list first.

**When to use `ApiProvider` vs `TtsRegistry`/`TranscribeRegistry`:**

- Use `TtsRegistry` / `TranscribeRegistry` for ephemeral local engines (subprocess, on-device model) that don't rely on the `llm_providers` credential DB.
- Use `ApiProvider` for cloud services that the user configures via the LLM-providers UI (API key stored in `llm_providers`, models managed via `tts_models` / `transcribe_models` CRUD).

All types used by `ApiProvider` (`LlmProviderRecord`, `LlmModelRecord`, `TtsModelRecord`, `TranscribeModelRecord`, `ImageGenerateModelRecord`, `RemoteLlmModelInfo`, `RemoteTtsModelInfo`, `RemoteTranscribeModelInfo`, `LlmStrength`) live in `core-api::provider` or their respective `core-api` modules. Plugin crates depend only on `core-api`.

---

## Plugin catalogue

| Plugin ID | Crate | Description |
|---|---|---|
| `honcho` | `crates/plugin-honcho` | Honcho long-term memory backend |
| `telegram_bot` | `crates/plugin-telegram-bot` | Private Telegram bot interface |
| `whisper_local` | `crates/plugin-transcribe-whisper-local` | Local STT via whisper.cpp |
| `tailscale_remote` | `crates/plugin-tailscale-remote` | Remote access via Tailscale mesh |
| `comfyui` | `crates/plugin-comfyui` | ComfyUI image generation workflows |
| `orpheus_tts_3b` | `crates/plugin-tts-orpheus-3b` | Local TTS via Orpheus 3B (Python subprocess) |
| `kokoro_tts` | `crates/plugin-tts-kokoro` | Local TTS via Kokoro ONNX (lightweight, multilingual) |
| `elevenlabs` | `crates/plugin-elevenlabs` | ElevenLabs cloud TTS and transcription (ApiProvider) |
