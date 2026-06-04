# Text-to-Speech Providers

Cloud TTS via any OpenAI-compatible audio speech endpoint, plus plugin-registered local engines.

---

## Architecture

```text
crates/core-api/src/tts.rs
  — TextToSpeech trait (provider interface)
  — TtsProvider trait (resolve active provider)
  — TtsRegistry trait (plugin write-side: register/unregister)

src/tts/
  mod.rs         — TtsModelRecord/Info, re-exports TextToSpeech/TtsProvider/TtsRegistry
  db.rs          — SQL layer for tts_models table
  manager.rs     — TtsManager (DB-aware, owns the table, impls TtsProvider + TtsRegistry)
  openai_tts.rs  — OpenAiTtsSynthesiser: impl TextToSpeech via HTTP JSON
```

Two kinds of providers coexist:

| Kind | Source | Example |
| ---- | ------ | ------- |
| **DB-backed** | `tts_models` table, built from `llm_providers` credentials | `OpenAiTtsSynthesiser` |
| **Plugin-registered** | Ephemeral — registered at runtime by plugins | `OrpheusTtsPlugin`, future: `KokoroPlugin`, `PiperPlugin` |

`get()` returns the first plugin provider (if any is running), then the first DB-backed provider ordered by `priority ASC`.

---

## Traits (crates/core-api)

```rust
// core_api::tts
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    fn id(&self)           -> &str;
    fn name(&self)         -> &str;
    fn description(&self)  -> Option<&str>;   // default None
    fn instructions(&self) -> Option<&str>;   // default voice style stored in DB
    async fn synthesize(&self, text: &str, instructions: Option<&str>) -> Result<Vec<u8>>;
}

/// Read-side used by callers to get the active provider.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn get(&self) -> Option<Arc<dyn TextToSpeech>>;
}

/// Write-side used by plugins to register/unregister ephemeral providers.
#[async_trait]
pub trait TtsRegistry: Send + Sync {
    async fn register(&self, provider: Arc<dyn TextToSpeech>);
    async fn unregister(&self, id: &str);
}
```

### `instructions` semantics

| Level | Where set | Precedence |
|-------|-----------|------------|
| **DB-level** | `tts_models.instructions` column | Default for this model config |
| **Call-time** | `synthesize(text, Some(override))` | Overrides DB-level for this call |

This lets the LLM (or a plugin) say "respond in a cheerful tone" on a per-turn basis without changing the model's default configuration.

---

## Manager API

```rust
// Async constructor — loads DB models on startup
TtsManager::new(pool: Arc<SqlitePool>) -> Result<Arc<Self>>

// Resolution
tts_manager.get().await    // → Option<Arc<dyn TextToSpeech>>  (plugins first, then DB)

// Plugin registration (ephemeral)
tts_manager.register(Arc::new(synthesiser)).await
tts_manager.unregister("kokoro_local").await

// DB-backed CRUD (called by REST API handlers)
tts_manager.add_model(record).await        // → Result<i64>
tts_manager.update_model(id, record).await
tts_manager.delete_model(id).await         // soft delete
tts_manager.get_model(id).await            // → Option<TtsModelRecord>

// Listings
tts_manager.list_models_info().await       // DB-backed only → Vec<TtsModelInfo>
tts_manager.list_all_info().await          // plugin + DB → Vec<TtsModelInfo>
```

---

## OpenAiTtsSynthesiser

Implemented in `src/tts/openai_tts.rs`.

Calls `POST {base_url}/audio/speech` with a JSON body:

| Field | Value |
|-------|-------|
| `model` | Provider model ID (e.g. `tts-1`, `tts-1-hd`, `gpt-4o-mini-tts`) |
| `input` | Text to synthesise |
| `voice` | `"alloy"` (default — overridable via `instructions`) |
| `response_format` | `"mp3"` |
| `instructions` | Optional natural-language style/tone/speed override |

Returns raw MP3 bytes.

### Supported providers

| Provider | `base_url` | Notes |
|----------|-----------|-------|
| OpenAI | `https://api.openai.com/v1` | Models: `tts-1`, `tts-1-hd`, `gpt-4o-mini-tts` |

---

## DB: tts_models table

```sql
CREATE TABLE tts_models (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id  INTEGER NOT NULL REFERENCES llm_providers(id),
    model_id     TEXT    NOT NULL,
    name         TEXT    NOT NULL UNIQUE,
    description  TEXT,                        -- human-readable, shown in UI
    instructions TEXT,                        -- default voice style / tone / speed
    priority     INTEGER NOT NULL DEFAULT 100,
    removed_at   TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE(provider_id, model_id)
)
```

---

## Plugin Registration

`TtsRegistry` is exposed on `PluginContext` as `ctx.tts_registry`. Plugin crates depend only on `core-api`.

```rust
use core_api::tts::TextToSpeech;

struct MyTtsSynth { /* ... */ }

#[async_trait]
impl TextToSpeech for MyTtsSynth {
    fn id(&self)   -> &str { "kokoro_local" }
    fn name(&self) -> &str { "Kokoro Local" }
    async fn synthesize(&self, text: &str, instructions: Option<&str>) -> Result<Vec<u8>> {
        // call local engine, return MP3 bytes
    }
}

// In Plugin::start() when enabled:
ctx.tts_registry.register(Arc::new(MyTtsSynth { ... })).await;

// In Plugin::stop():
ctx.tts_registry.unregister("kokoro_local").await;
```

---

## Orpheus TTS 3B (`plugin-tts-orpheus-3b`)

Local, on-device TTS using the Orpheus 3B model. Runs a Python subprocess for inference.

**Crate:** `crates/plugin-tts-orpheus-3b/`  
**Plugin ID:** `orpheus_tts_3b`

### How it works

The Python inference server (`orpheus_server.py`) is **embedded in the plugin binary** via `include_str!`. On start, the plugin writes it to `models/orpheus-3b/orpheus_server.py` and spawns it. The server prints `PORT:<n>` to stdout when ready; the plugin reads that port and registers itself as a `TextToSpeech` provider. On stop, the subprocess is killed and the script file is removed.

### Setup

```text
set_secret("HUGGINGFACE_TOKEN", "hf_...")
configure_plugin("orpheus_tts_3b", {"quantization": "int8", "voice": "tara"})
toggle_plugin("orpheus_tts_3b", true)
```

### Config

| Field          | Values                                               | Default |
|----------------|------------------------------------------------------|---------|
| `quantization` | none / int8 / int4                                   | int8    |
| `voice`        | tara / dan / leah / zac / zoe / mia / julia / leo    | tara    |

---

## When to Update This File

- A new concrete `TextToSpeech` implementation is added
- `tts_models` schema changes
- A provider gains or loses TTS support
