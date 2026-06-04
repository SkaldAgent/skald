use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::image_generate::ImageGeneratorManager;
use crate::tools::{Tool, ToolCategory};

// ── image_generate_providers_list ─────────────────────────────────────────────

pub struct ImageGenerateProvidersList {
    pub mgr: Arc<ImageGeneratorManager>,
}

impl Tool for ImageGenerateProvidersList {
    fn name(&self) -> &str { "image_generate_providers_list" }
    fn category(&self) -> ToolCategory { ToolCategory::Introspection }

    fn description(&self) -> &str {
        "List all registered image generation providers. \
         Returns an array of {id, name} objects. \
         Use the id with image_generate to pick a provider."
    }

    fn parameters_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn execute(&self, _args: Value) -> Result<String> {
        let providers = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.mgr.list())
        });
        Ok(serde_json::to_string_pretty(&providers)?)
    }
}

// ── image_generate ────────────────────────────────────────────────────────────

pub struct ImageGenerateTool {
    pub mgr: Arc<ImageGeneratorManager>,
}

impl Tool for ImageGenerateTool {
    fn name(&self) -> &str { "image_generate" }
    fn category(&self) -> ToolCategory { ToolCategory::Config }

    fn description(&self) -> &str {
        "Generate an image from a text prompt. \
         Blocks until the image is ready, then returns the local path and a web URL."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["provider_id", "prompt"],
            "properties": {
                "provider_id": {
                    "type":        "string",
                    "description": "ID of the image generation provider (from image_generate_providers_list)"
                },
                "prompt": {
                    "type":        "string",
                    "description": "Text prompt describing the image to generate"
                },
                "extra_params": {
                    "type":        "object",
                    "description": "Optional provider-specific parameters (e.g. width, height, steps). \
                                    See extra_params_schema in image_generate_providers_list for valid fields."
                }
            }
        })
    }

    fn execute(&self, args: Value) -> Result<String> {
        let provider_id = args["provider_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("missing provider_id"))?
            .to_string();
        let prompt = args["prompt"].as_str()
            .ok_or_else(|| anyhow::anyhow!("missing prompt"))?
            .to_string();
        let extra_params = match &args["extra_params"] {
            Value::Object(_) => Some(args["extra_params"].clone()),
            _                => None,
        };

        let mgr = Arc::clone(&self.mgr);
        let (path, url) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(mgr.generate(&provider_id, &prompt, extra_params.as_ref()))
        })?;

        Ok(json!({ "path": path, "url": url }).to_string())
    }
}
