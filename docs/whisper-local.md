# WhisperLocal Plugin

Local Speech-to-Text via [whisper.cpp](https://github.com/ggerganov/whisper.cpp), Metal-accelerated on Apple Silicon.
Implemented in pure Rust using the `whisper-rs` crate — no Python involved.

---

## Setup

### 1. Download a GGML model

```sh
mkdir -p models
curl -L -o models/ggml-large-v3.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin
```

Other available sizes (smaller = faster, less accurate):

| Model | Size | Notes |
|---|---|---|
| `ggml-tiny.bin` | ~75 MB | Very fast, lower accuracy |
| `ggml-base.bin` | ~142 MB | Good balance for testing |
| `ggml-small.bin` | ~466 MB | Good accuracy |
| `ggml-medium.bin` | ~1.5 GB | High accuracy |
| `ggml-large-v3.bin` | ~3.1 GB | Best accuracy, recommended |
| `ggml-large-v3-turbo.bin` | ~1.6 GB | large-v3 speed-optimised |

All models: `https://huggingface.co/ggerganov/whisper.cpp`

### 2. Configure `config.yml`

```yaml
plugins:
  whisper_local:
    model: "models/ggml-large-v3.bin"
    language: "it"   # BCP-47 code, or "auto" for detection
```

### 3. Build

The first `cargo build` compiles whisper.cpp (a few minutes). Subsequent builds are cached.

---

## How it works

```
Telegram voice message (OGG/Opus)
  │
  ▼ ffmpeg → 16 kHz mono WAV
  ▼ hound  → Vec<f32> PCM samples
  ▼ whisper.cpp (Metal GPU) → text
  │
  ▼ forwarded to LLM as a normal text message
```

Audio conversion uses the system `ffmpeg` binary (must be installed: `brew install ffmpeg`).
Inference runs on Apple Silicon GPU via Metal. Falls back to CPU if Metal is unavailable.

---

## Integration with TranscribeManager

`WhisperLocalPlugin` does **not** expose itself as `Arc<dyn Transcribe>` directly. At `start()` it registers a lightweight `WhisperLocalTranscriber` handle into `skald.transcribe_manager`; at `stop()` it deregisters it. Callers never reference the plugin type — they ask the manager:

```rust
if let Some(t) = skald.transcribe_manager.get().await {
    let text = t.transcribe(audio, "ogg").await?;
}
```

See [plugins.md](plugins.md) for the `TranscribeManager` API and the `Transcribe` trait.

---

## When to Update This File

- The audio conversion pipeline changes
- Default recommended models change
- Registration/deregistration logic in `start()`/`stop()` changes
