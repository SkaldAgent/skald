use anyhow::Result;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolDescriptionLength, truncate_label, MAX_LABEL_SHORT, MAX_LABEL_FULL};
use super::read_to_string;

pub struct ReadFile;

impl ReadFile {
    pub fn new() -> Self { Self }
}

impl Tool for ReadFile {
    fn name(&self) -> &str { "read_file" }
    fn category(&self) -> crate::tools::ToolCategory { crate::tools::ToolCategory::Filesystem }

    fn description(&self) -> &str {
        "Read the content of any file, optionally limited to a line range. \
         Relative paths are resolved from the project root; absolute paths (starting with /) are used as-is. \
         Returns text with 1-based line numbers prefixed as '  N | '. \
         When calling edit_file, copy the text after '| ' exactly. \
         Use start_line/end_line to read large files in chunks instead of loading the whole file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type":        "string",
                    "description": "File path. Relative to project root, or absolute (e.g. /etc/hosts)."
                },
                "start_line": {
                    "type":        "integer",
                    "description": "First line to read (1-based, inclusive). Omit to start from the beginning."
                },
                "end_line": {
                    "type":        "integer",
                    "description": "Last line to read (1-based, inclusive). Omit to read to the end of the file."
                }
            },
            "required": ["path"]
        })
    }

    fn describe(&self, args: &Value, length: ToolDescriptionLength) -> String {
        let path = args["path"].as_str().unwrap_or("?");
        match length {
            ToolDescriptionLength::Short => {
                truncate_label(&format!("read_file `{path}`"), MAX_LABEL_SHORT)
            }
            ToolDescriptionLength::Full => {
                let range = match (args["start_line"].as_u64(), args["end_line"].as_u64()) {
                    (Some(s), Some(e)) => format!(" lines {s}-{e}"),
                    (Some(s), None)    => format!(" from line {s}"),
                    (None,    Some(e)) => format!(" to line {e}"),
                    _                  => String::new(),
                };
                truncate_label(&format!("read_file `{path}`{range}"), MAX_LABEL_FULL)
            }
        }
    }

    fn execute(&self, args: Value) -> Result<String> {
        let user_path = args["path"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: path"))?;
        let content = read_to_string(user_path)?;
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let start = args["start_line"].as_u64()
            .map(|n| (n as usize).saturating_sub(1))
            .unwrap_or(0);
        let end = args["end_line"].as_u64()
            .map(|n| (n as usize).min(total))
            .unwrap_or(total);

        if start >= total && total > 0 {
            return Ok(format!("(file has only {total} lines; start_line {start_line} is out of range)",
                start_line = start + 1));
        }

        let end = end.max(start);

        let width = total.to_string().len().max(3);
        let numbered = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>width$} | {line}", start + i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(numbered)
    }
}
