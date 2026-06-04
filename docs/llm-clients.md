# LLM Clients

## Workspace Location

The `ChatbotClient` trait and all provider implementations live in the standalone crate `crates/llm-client` (no dependencies on the main crate). `src/chatbot/mod.rs` is a thin re-export layer. `LoggingChatbotClient` (`src/chatbot/logging.rs`) remains in the main crate because it depends on `sqlx`.

---

## ChatbotClient Trait

```rust
#[async_trait]
pub trait ChatbotClient: Send + Sync {
    async fn chat(&self, messages: &[Message], options: &ChatOptions) -> Result<ChatResponse>;
    async fn chat_with_tools(&self, messages: &[Value], tools: &[Value], options: &ChatOptions) -> Result<LlmTurn>;
    async fn chat_with_tools_raw(&self, messages: &[Value], tools: &[Value], options: &ChatOptions) -> Result<(LlmTurn, Option<LlmRawMeta>)>;
}
```

Only `AnthropicClient` and `OpenAiClient` implement native tool support (`chat_with_tools`). Other clients have a default fallback that strips tool definitions and calls `chat()` instead.

`chat_with_tools_raw` is used by the logging wrapper: it returns the same `LlmTurn` plus raw HTTP request/response metadata (`LlmRawMeta`). `AnthropicClient`, `OpenAiClient`, and `LmStudioClient` override it; all others fall back to calling `chat_with_tools` with no metadata.

`ChatOptions` carries two optional fields — `session_id` and `stack_id` — set by the LLM loop for correlation. Providers ignore them; only `LoggingChatbotClient` reads them.

---

## Transparent Request Logging

`LoggingChatbotClient` (`src/chatbot/logging.rs`) is a `ChatbotClient` wrapper that intercepts every `chat_with_tools` call:

1. Calls `inner.chat_with_tools_raw(...)` to capture the HTTP wire data.
2. Spawns a **fire-and-forget** `tokio::spawn` to insert a row into `llm_requests`.
3. Returns the `LlmTurn` to the caller unchanged.

The LLM loop is fully unaware — it holds an `Arc<dyn ChatbotClient>` and calls `chat_with_tools` as usual. The wrapper is applied in `LlmManager::build_entry` when `request_log_enabled = true` (set from `config.yml → llm.request_log.enabled`).

What is logged per row: full request body (provider-specific format), request headers (api-key redacted), full response body, response headers, token counts, round-trip duration, session/stack ID.

A background task (boot + every hour) deletes rows older than `retention_days` (default 14).

`LlmTurn` return variants:

- `Message(ChatResponse)` — final text answer
- `ToolCalls { content, calls, input_tokens, output_tokens, reasoning_content }` — one or more tool calls requested

Both variants carry an optional `reasoning_content: Option<String>`. Populated only by providers that return chain-of-thought (currently DeepSeek thinking mode). Saved to `chat_history.reasoning_content` and echoed back on subsequent turns — see *Reasoning Content / DeepSeek Thinking Mode* below.

---

## Supported Providers

| Enum variant  | Client struct     | api_key required | Default base_url                   | Prompt cache                   |
| ------------- | ----------------- | ---------------- | ---------------------------------- | ------------------------------ |
| `LmStudio`    | `LmStudioClient`  | No               | `http://localhost:1234/v1`         | ❌                             |
| `Ollama`      | `OllamaClient`    | No               | `http://localhost:11434`           | ❌                             |
| `OpenAi`      | `OpenAiClient`    | **Yes**          | `https://api.openai.com/v1`        | ❌                             |
| `OpenRouter`  | `OpenAiClient`    | **Yes**          | `https://openrouter.ai/api/v1`     | ✅ `anthropic/*` only          |
| `Anthropic`   | `AnthropicClient` | **Yes**          | `https://api.anthropic.com`        | ❌ (planned)                   |
| `DeepSeek`    | `OpenAiClient`    | **Yes**          | `https://api.deepseek.com/v1`      | ✅ automatic (see below)       |

`OpenRouter` and `DeepSeek` reuse `OpenAiClient` with different base URLs.

---

## Prompt Caching (KV Cache)

When `LlmEntry.prompt_cache = true` (currently set only for `OpenRouter`), the agent enables Anthropic-compatible KV caching on every request:

### What is sent

1. **`anthropic-beta: prompt-caching-2024-07-31` HTTP header** — tells OpenRouter/Anthropic to activate the caching feature.

2. **Static system message tagged for caching** — `build_openai_messages` emits the first system message (AGENT.md + memory files + `extra_system_static` + MCP list) as a content array with `cache_control: {"type": "ephemeral"}` on the single block. This is the KV cache prefix.

3. **Last tool tagged** — the final entry in the `tools` array receives `cache_control: {"type": "ephemeral"}`, caching the entire tool list as part of the prefix.

### Message order and cache stability

The full message array is structured so the stable prefix is as long as possible (see *Context Building* in [session.md](session.md)):

```text
[static system — cached]  [scratchpad?]  [summary?]  [conversation]  [dynamic tail]  [tail reminder]
```

The dynamic tail (Honcho memories + date/time) is placed **after** the conversation, so it never shortens the cacheable prefix. Scratchpad is a separate message so a mid-turn write only invalidates that small block, not the large static prefix.

### Cache TTL

Anthropic's `ephemeral` cache has a sliding TTL of ~5 minutes (extended by each hit). A cache hit is reported in the response as `cache_read_input_tokens` in the usage block.

### DeepSeek automatic KV cache

DeepSeek's disk KV cache is **prefix-based and fully automatic** — no explicit markers or special headers are required. Because the static system message is always the first entry in the message array, it becomes the stable cache prefix on every turn.

`prompt_cache = false` for the `DeepSeek` provider: no Anthropic-style `cache_control` blocks are injected (DeepSeek does not understand them). The cache operates transparently. DeepSeek reports cache hits in the response under `usage.prompt_cache_hit_tokens` / `usage.prompt_cache_miss_tokens` (visible in the raw request log).

#### ⚠️ The dynamic tail and cache invalidation

DeepSeek's KV cache compares requests **token by token from position 0**. If any token differs at position N, everything from N onward is recomputed — there is no partial matching inside the sequence.

The dynamic tail (date/time + Honcho memories) is injected as a system message at the end of the message array, after the conversation history. Because it is placed *after* the conversation, it doesn't shorten the cacheable prefix for the system message and tools. However, the **exact timestamp** (`2026-05-28T10:56:34+02:00`) changes every second. This means the stored KV entry from the previous request ends with `[..conversation..][dyn_tail_T1]`, while the new request has `[..conversation..][new_user_msg][dyn_tail_T2]`. The break point occurs right after the last assistant message: everything beyond it must be recomputed.

In practice this means: **without timestamp rounding, only the static system message and tools are effectively cached.** The conversation history accumulates in the cache prefix, but the always-changing timestamp prevents the prefix from extending into the tail message of the stored entry.

**Observed impact (production data):**

| Configuration | `prompt_cache_hit_tokens` | `prompt_cache_miss_tokens` |
| --- | --- | --- |
| Exact timestamp (default before fix) | ~6,144 | ~21,583 |
| `round_minutes: 15` | ~38,272 | ~830 |

With rounding, the timestamp string stays byte-identical for up to 15 minutes, letting the full conversation + tools accumulate in the cache prefix. The remaining ~830 miss tokens represent only the current user message (unavoidably new on every request).

### `llm.datetime` — timestamp injection config

Controlled by `config.yml → llm.datetime`:

```yaml
llm:
  datetime:
    enabled: true        # false = omit date/time from context entirely
    round_minutes: 15    # round down to nearest N-minute boundary
                         # e.g. 10:56 → 10:50 with round_minutes: 10
                         # omit for exact timestamp (hurts KV cache on prefix-based providers)
```

`round_minutes` is the primary tuning knob for cache efficiency on DeepSeek and any other prefix-based KV cache provider. The trade-off is precision: the LLM sees a timestamp that may be up to `round_minutes` minutes in the past. For most conversational uses this is imperceptible; for time-critical tasks (cron triggers, calendar scheduling) prefer a smaller value or `null`.

The default in `default.config.yaml` is `round_minutes: 15` — a safe value that gives near-100% cache hit rates in typical conversations while keeping the timestamp accurate to within a quarter-hour.

### Future: Anthropic direct

`AnthropicClient` does not yet support `prompt_cache`. The implementation is different: the `system` parameter must be sent as a JSON array of content blocks rather than a plain string. Tracked as a future improvement.

---

---

## Reasoning Content / DeepSeek Thinking Mode

When DeepSeek is configured with `"thinking": {"type": "enabled"}` in `extra_params`, each response includes a `reasoning_content` field alongside the normal `content`. This is the model's chain-of-thought.

**DeepSeek requires that `reasoning_content` be echoed back in the assistant message on subsequent turns.** Omitting it causes a `400 invalid_request_error`.

### How it works

1. `OpenAiClient.chat_with_tools_raw` reads `message.reasoning_content` from the response and propagates it through `LlmTurn`.
2. `llm_loop` saves it to `chat_history.reasoning_content` alongside the assistant's text content.
3. `build_openai_messages` includes `reasoning_content` in the reconstructed assistant message whenever the field is non-null.

All other providers always return `reasoning_content: None`; the field is simply absent from their assistant messages in the history.

---

## LlmStrength Enum

Ordered (weakest → strongest): `VeryLow` < `Low` < `Average` < `High` < `VeryHigh`

Used by AUTO selection and `call_agent` to match agents to capable models.

---

## AUTO Selection Algorithm

When `client = "auto"` or no client is specified, `LlmManager::select()` runs four passes in order, returning the first match:

1. Not-Down + strength ≥ required + scope matches
2. Not-Down + strength ≥ required (scope relaxed)
3. Any Not-Down model
4. **Emergency fallback**: strongest model even if Down (logs a `WARN`)

Models are ordered by `priority ASC` in the DB; lower number = tried first.

---

## Health Tracking

| Threshold                    | Status              |
| ---------------------------- | ------------------- |
| `consecutive_failures >= 3`  | `Degraded`          |
| `consecutive_failures >= 5`  | `Down`              |
| Next success                 | Reset to `Healthy`  |

`mark_failure()` is called by `run_agent_turn` on LLM call errors. `mark_success()` is called on every successful response. Health state is preserved across `reload()` calls (e.g. after adding a new model).

---

## Automatic LLM Failover

When the primary model returns a retriable error (5xx, network error, 429), `run_agent_turn` automatically tries the next available model — up to **3 attempts per round**.

**Retry logic**:

- A fresh `tried_this_round` list is built at the start of every round.
- On error, `is_retriable_llm_error()` decides whether to try again. Client errors (400/401/403/404/422) are **not** retried — the request itself is invalid.
- `select_excluding(&tried)` picks the next model, applying the same scope/strength rules as AUTO selection but skipping already-tried ones.
- If a different model uses different `prompt_cache` settings, messages are rebuilt before the retry.
- `cur_name`/`cur_llm` persist for the rest of the turn once switched, so subsequent rounds use the new model without re-trying the failed one.

**Events emitted**:

| Event | When | Who reacts |
| --- | --- | --- |
| `model_fallback` | Each successful switch | Frontend shows an inline info note |
| `llm_failed` | All attempts exhausted | Frontend shows error + `_waiting = false`; Telegram sends a message |

Telegram ignores `model_fallback` (silent retry) but sends an error message for `llm_failed`, matching the same behaviour as `Error`.

---

## Valid Scope Values

`basic`, `writing`, `coding`, `reasoning`, `math`, `search`

Defined by convention; any string is accepted by the DB. Agents declare `scope` in `meta.json`; models declare matching scopes in the DB.

---

## Extra Params

Each model can store an optional `extra_params` JSON object. Its top-level keys are **merged into the request body** before every API call, overriding any default key with the same name.

Only `OpenAiClient` (covers `OpenAi` and `OpenRouter` providers) applies extra params. `AnthropicClient` ignores them.

**Example — DeepSeek thinking mode (native DeepSeek provider):**

```json
{ "thinking": {"type": "enabled"}, "reasoning_effort": "high" }
```

`reasoning_effort` accepts `"low"`, `"medium"`, or `"high"`. Only supported by DeepSeek reasoning models (e.g. `deepseek-reasoner`); sending these params to non-reasoning models returns a 400.

**Example — DeepSeek reasoning effort on OpenRouter:**

```json
{ "reasoning": { "effort": "high" } }
```

Set via the model edit modal in the LLM Models UI, or via `PUT /api/llm/models/{id}` with `extra_params` in the JSON body.

---

## Model Metadata Fields

Each model record now stores additional metadata beyond the core LLM configuration:

| Field | Type | Source | Runtime use |
|---|---|---|---|
| `context_length` | `Option<i64>` | Provider catalog sync or manual input | Compaction threshold calculation, `max_tokens` limiting |
| `max_output_tokens` | `Option<i64>` | Provider catalog sync or manual input | Future: set `max_tokens` on LLM calls (currently `None`) |
| `knowledge_cutoff` | `Option<String>` | Provider catalog sync or manual input | Future: inject into system prompt |
| `capabilities` | `Vec<String>` | Provider catalog sync or manual input | Filtering by model feature (vision, function_calling, etc.) |

All fields are optional (`NULL` in the DB). When the provider catalog reports them,
they are automatically synced to existing DB records by `list_provider_models()`.
Manual values set via the API or UI take precedence when the provider does not
report a particular field (the sync uses `COALESCE` — only non-NULL catalog values
overwrite).

---

## LLM CRUD

All mutations go through `LlmManager` (not direct DB writes) because each operation calls `reload()` to rebuild the in-memory state:

- `add_provider()` / `update_provider()` / `delete_provider()`
- `add_model()` / `update_model()` / `delete_model()`

Setting `is_default = true` on a model automatically clears the flag on all others.

**Soft delete:** `delete_provider()` and `delete_model()` never issue `DELETE` statements. They set `removed_at = datetime('now')` on the row. Deleting a provider also cascades to all its models and clears the provider's `api_key`. Removed rows are excluded from `load_all_providers()` / `load_all_models()` and therefore from the in-memory state and AUTO selector. The `id` values remain valid as FK references in `chat_history.model_db_id`.

---

## ProviderCaps — Model Types

Each provider declares which model kinds it supports via `ProviderCaps::supported_types() -> &'static [ModelType]`. This is hardcoded per implementation — not stored in the DB.

| Provider   | `supported_types()`                    |
| ---------- | -------------------------------------- |
| OpenRouter | `[Llm, Transcribe, ImageGenerate]`     |
| OpenAi     | `[Llm, Transcribe, ImageGenerate]`     |
| Anthropic  | `[Llm]`                                |
| Ollama     | `[Llm]`                                |
| LmStudio   | `[Llm]`                                |
| DeepSeek   | `[Llm]`                                |

`supported_types` is included in the `GET /api/llm/providers` response so the frontend can filter the provider dropdown when adding transcription or image generation models.

---

## ProviderCaps — Remote Model Catalog

`src/llm/providers/` defines a second trait orthogonal to `ChatbotClient`, used to query provider metadata (model list, pricing, per-model info):

```rust
#[async_trait]
pub trait ProviderCaps: Send + Sync {
    /// List all models available on this provider, with metadata.
    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>>;

    /// Fetch metadata for a single model by its provider-specific ID.
    /// Default impl returns `None` (provider does not support per-model queries).
    async fn model_info(&self, model_id: &str) -> Result<Option<RemoteModelInfo>> {
        Ok(None)
    }
}
```

`RemoteModelInfo` fields: `id`, `name`, `context_length`, `max_completion_tokens`,
`knowledge_cutoff`, `capabilities`, `price_avg_per_million` (USD/M tokens, avg of prompt+completion).

| Provider | Implementation | `list_models()` | `model_info()` |
| --- | --- | --- | --- |
| `OpenRouter` | `OpenRouterProvider` | `GET /api/v1/models` | `GET /api/v1/models/{id}` |
| `Ollama` | `OllamaProvider` | `GET /api/tags` | `POST /api/show` |
| `Anthropic` | `AnthropicProvider` | `None` | `GET /v1/models/{id}` |
| `DeepSeek` | `DeepSeekProvider` | `GET /models` | `None` |
| `LmStudio` | `LmStudioProvider` | `GET /v1/models` | `None` |
| `OpenAi` | `OpenAiProvider` | `None` | `None` |

Instances are created on-demand via `providers::build_caps(record)`.

### Model Catalog Cache

`LlmManager` caches `list_models()` results in memory, keyed by `provider_id`, with a **24-hour TTL**. The cache is discarded on process restart.

### Per-Model Metadata Cache

When `LlmManager::resolve()` is called, it lazily fetches `model_info()` for the resolved model
if the per-model cache is missing or older than **1 hour**. The `context_length` from the fresh
metadata is then propagated to the live `LlmEntry` in the model slot so subsequent turns use
the updated value.

Cache flow:

- Fast path: read lock on `model_meta_cache` → hit + fresh → return immediately.
- Miss / stale: fetch `model_info()` from the provider → update cache → update `LlmEntry.context_length`.
- Network failure: the old cached value (or DB value) is preserved — the error is silently ignored.

This ensures the compactor and any future `max_tokens` logic always have a reasonably current
`context_length` without blocking the first turn of the session.

```text
LlmManager::list_provider_models(provider_id)
  → cache hit  (< 24h old) → return cached Vec<RemoteModelInfo>
  → cache miss / expired   → fetch via ProviderCaps, store, return
```

API endpoint: `GET /api/llm/providers/{id}/models`

Used by the frontend "Add Model" wizard to populate the searchable model picker for OpenRouter, Ollama, and LM Studio providers.

---

## When to Update This File

- A new provider type is added to `LlmProvider` enum
- The AUTO selection algorithm changes
- Health thresholds (`FAILURE_DEGRADED`, `FAILURE_DOWN`) change
- `ProviderCaps` gains new methods or a new provider gets an implementation
