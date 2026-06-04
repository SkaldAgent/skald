/// WhisperLocalPlugin — local Speech-to-Text via whisper.cpp (Metal-accelerated on Apple Silicon).
///
/// Audio (any format) is first converted to 16 kHz mono WAV by ffmpeg, then fed to
/// whisper.cpp through the `whisper-rs` crate. No Python involved.
///
/// The model must be a GGML `.bin` file. Download with:
///   curl -L -o models/ggml-large-v3.bin \
///     https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{debug, info, warn};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use core_api::plugin::{Plugin, PluginContext};
use core_api::transcribe::{Transcribe, TranscribeRegistry};

// ── WhisperLocalPlugin ────────────────────────────────────────────────────────

pub struct WhisperLocalPlugin {
    /// Path to the GGML model file — behind a Mutex so reload() can swap it.
    model_path:           tokio::sync::Mutex<PathBuf>,
    /// BCP-47 language code — can be changed at runtime without model reload.
    language:             tokio::sync::Mutex<String>,
    /// Loaded model context — None until start(), reset to None on stop().
    ctx:                  tokio::sync::Mutex<Option<Arc<WhisperContext>>>,
    running:              Arc<AtomicBool>,
    /// Kept so stop() can deregister without needing the full context.
    transcribe_registry:  tokio::sync::Mutex<Option<Arc<dyn TranscribeRegistry>>>,
}

impl WhisperLocalPlugin {
    pub fn new() -> Self {
        Self {
            model_path:          tokio::sync::Mutex::new(PathBuf::new()),
            language:            tokio::sync::Mutex::new("auto".to_string()),
            ctx:                 tokio::sync::Mutex::new(None),
            running:             Arc::new(AtomicBool::new(false)),
            transcribe_registry: tokio::sync::Mutex::new(None),
        }
    }
}

impl Default for WhisperLocalPlugin {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Plugin for WhisperLocalPlugin {
    fn id(&self)          -> &str { "whisper_local" }
    fn name(&self)        -> &str { "Whisper Local" }
    fn description(&self) -> &str { "Local STT via whisper.cpp (Metal-accelerated)" }
    fn is_running(&self) -> bool  { self.running.load(Ordering::Relaxed) }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "model": {
                    "type":        "string",
                    "title":       "Model path",
                    "description": "Path to GGML .bin file (e.g. models/ggml-large-v3.bin)"
                },
                "language": {
                    "type":        "string",
                    "title":       "Language",
                    "description": "BCP-47 code (e.g. 'it', 'en') or 'auto' for auto-detect",
                    "default":     "auto"
                }
            },
            "required": ["model"]
        })
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn as_arc_any(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> { self }

    async fn reload(&self, enabled: bool, config: Value, ctx: PluginContext) -> Result<()> {
        let new_model = config["model"].as_str().unwrap_or("").to_string();
        let new_lang  = config["language"].as_str().unwrap_or("auto").to_string();
        let old_model = self.model_path.lock().await.to_string_lossy().to_string();
        let is_running = self.is_running();

        match (enabled, is_running) {
            (true, false) => {
                anyhow::ensure!(!new_model.is_empty(),
                    "whisper_local: cannot start — `model` is missing from config");
                *self.model_path.lock().await = PathBuf::from(&new_model);
                *self.language.lock().await   = new_lang;
                self.start(ctx).await?;
            }
            (false, true) => {
                self.stop().await?;
            }
            (true, true) => {
                if new_model != old_model {
                    info!("whisper_local: model path changed — reloading");
                    self.stop().await?;
                    *self.model_path.lock().await = PathBuf::from(&new_model);
                    *self.language.lock().await   = new_lang;
                    self.start(ctx).await?;
                } else if new_lang != *self.language.lock().await {
                    info!(language = %new_lang, "whisper_local: language updated (no model reload)");
                    *self.language.lock().await = new_lang;
                }
            }
            (false, false) => {}
        }
        Ok(())
    }

    async fn start(&self, ctx: PluginContext) -> Result<()> {
        if self.running.load(Ordering::Relaxed) { return Ok(()); }

        // Redirect all whisper.cpp / ggml C-level log output through Rust callbacks.
        // Since we don't enable the `log_backend` or `tracing_backend` features,
        // the trampolines are no-ops — this silences all init/model-load chatter.
        // The call is idempotent (backed by std::sync::Once).
        whisper_rs::install_logging_hooks();

        let model_path = self.model_path.lock().await.clone();

        anyhow::ensure!(
            model_path.exists(),
            "whisper_local: model file not found at '{}'. \
             Download a GGML model and set the path via the plugins API.",
            model_path.display()
        );

        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);

        let path_str = model_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8"))?
            .to_string();

        let whisper_ctx = tokio::task::spawn_blocking(move || {
            WhisperContext::new_with_params(&path_str, params)
                .map_err(|e| anyhow::anyhow!("failed to load whisper model: {e:?}"))
        })
        .await??;

        *self.ctx.lock().await = Some(Arc::new(whisper_ctx));
        self.running.store(true, Ordering::Relaxed);
        info!(model = %model_path.display(), "whisper_local plugin started");

        *self.transcribe_registry.lock().await = Some(Arc::clone(&ctx.transcribe_registry));
        ctx.transcribe_registry.register(Arc::new(WhisperLocalTranscriber {
            ctx:      self.ctx.lock().await.clone().unwrap(),
            language: self.language.lock().await.clone(),
        })).await;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        *self.ctx.lock().await = None;
        info!("whisper_local plugin stopped");
        if let Some(reg) = self.transcribe_registry.lock().await.take() {
            reg.unregister("whisper_local").await;
        }
        Ok(())
    }
}

// ── WhisperLocalTranscriber ───────────────────────────────────────────────────
//
// Lightweight handle registered in TranscribeManager. Cloned from the plugin's
// ctx + language at start() time so it can be passed as Arc<dyn Transcribe>
// without holding a reference back to the plugin.

struct WhisperLocalTranscriber {
    ctx:      Arc<WhisperContext>,
    language: String,
}

#[async_trait]
impl Transcribe for WhisperLocalTranscriber {
    fn id(&self) -> &str { "whisper_local" }

    async fn transcribe(&self, audio: Vec<u8>, format: &str) -> Result<String> {
        let ctx      = Arc::clone(&self.ctx);
        let language = self.language.clone();

        debug!(bytes = audio.len(), format, "whisper_local: transcribing audio");

        let pcm = audio_to_pcm_f32(audio, format).await?;

        tokio::task::spawn_blocking(move || {
            let mut state = ctx.create_state()
                .map_err(|e| anyhow::anyhow!("whisper state error: {e:?}"))?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some(&language));
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            state.full(params, &pcm)
                .map_err(|e| anyhow::anyhow!("whisper inference error: {e:?}"))?;

            let n = state.full_n_segments();

            let text = (0..n)
                .map(|i| {
                    state.get_segment(i)
                        .ok_or_else(|| anyhow::anyhow!("whisper: segment {i} out of range"))
                        .and_then(|s| s.to_str()
                            .map(str::to_string)
                            .map_err(|e| anyhow::anyhow!("whisper segment text error: {e:?}")))
                })
                .collect::<Result<Vec<_>>>()?
                .join(" ")
                .trim()
                .to_string();

            info!(chars = text.len(), segments = n, "whisper_local: transcription complete");
            Ok(text)
        })
        .await?
    }
}

// ── Audio conversion ──────────────────────────────────────────────────────────
//
// Uses ffmpeg (assumed installed) to decode any audio format to 16 kHz mono WAV,
// then reads it with hound. whisper.cpp requires f32 PCM at exactly 16 kHz mono.

async fn audio_to_pcm_f32(audio: Vec<u8>, format: &str) -> Result<Vec<f32>> {
    let pid = std::process::id();
    let tmp_in  = std::env::temp_dir().join(format!("whisper_in_{pid}.{format}"));
    let tmp_out = std::env::temp_dir().join(format!("whisper_out_{pid}.wav"));

    tokio::fs::write(&tmp_in, &audio).await?;

    let result = tokio::process::Command::new("ffmpeg")
        .args([
            "-y", "-i", tmp_in.to_str().unwrap(),
            "-ar", "16000",
            "-ac", "1",
            tmp_out.to_str().unwrap(),
        ])
        .output()
        .await;

    tokio::fs::remove_file(&tmp_in).await.ok();

    let output = result?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg audio conversion failed: {stderr}");
    }

    let wav_bytes = tokio::fs::read(&tmp_out).await?;
    tokio::fs::remove_file(&tmp_out).await.ok();

    tokio::task::spawn_blocking(move || {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav_bytes))?;
        let spec = reader.spec();

        let pcm: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                reader.samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / (1i32 << (bits - 1)) as f32))
                    .collect::<Result<_, _>>()?
            }
            hound::SampleFormat::Float => {
                reader.samples::<f32>()
                    .collect::<Result<_, _>>()?
            }
        };

        if pcm.is_empty() {
            warn!("whisper_local: decoded PCM is empty");
        }
        Ok::<Vec<f32>, anyhow::Error>(pcm)
    })
    .await?
}
