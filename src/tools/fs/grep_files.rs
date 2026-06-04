use anyhow::Result;
use regex::Regex;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolDescriptionLength, truncate_label, MAX_LABEL_SHORT, MAX_LABEL_FULL};
use super::resolve;

pub struct GrepFiles;

impl GrepFiles {
    pub fn new() -> Self { Self }
}

impl Tool for GrepFiles {
    fn name(&self) -> &str { "grep_files" }
    fn category(&self) -> crate::tools::ToolCategory { crate::tools::ToolCategory::Filesystem }

    fn description(&self) -> &str {
        "Search for a regex pattern across files in a directory (or a single file). \
         Returns matching lines with file path and 1-based line number. \
         Binary files and common non-text directories (target/, .git/, node_modules/, .venv/) are skipped. \
         Use this instead of search_file when you need to search across multiple files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory or file to search. Relative to project root, or absolute."
                },
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for (case-insensitive by default)."
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "If true, match is case-sensitive. Default: false.",
                    "default": false
                },
                "include_glob": {
                    "type": "string",
                    "description": "Optional glob to restrict which files are searched, e.g. '*.rs' or '*.py'."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Stop after this many matches (default 100).",
                    "default": 100
                }
            },
            "required": ["path", "pattern"]
        })
    }

    fn describe(&self, args: &Value, length: ToolDescriptionLength) -> String {
        let pattern = args["pattern"].as_str().unwrap_or("?");
        match length {
            ToolDescriptionLength::Short => {
                truncate_label(&format!("grep_files `{pattern}`"), MAX_LABEL_SHORT)
            }
            ToolDescriptionLength::Full => {
                let path = args["path"].as_str().unwrap_or(".");
                truncate_label(&format!("grep_files `{pattern}` in {path}"), MAX_LABEL_FULL)
            }
        }
    }

    fn execute(&self, args: Value) -> Result<String> {
        let user_path     = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing: path"))?;
        let pattern       = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("Missing: pattern"))?;
        let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);
        let include_glob  = args["include_glob"].as_str();
        let max_results   = args["max_results"].as_u64().unwrap_or(100) as usize;

        let re = {
            let pat = if case_sensitive {
                pattern.to_string()
            } else {
                format!("(?i){pattern}")
            };
            Regex::new(&pat).map_err(|e| anyhow::anyhow!("Invalid regex: {e}"))?
        };

        let glob_pattern = include_glob.map(|g| {
            glob::Pattern::new(g).ok()
        }).flatten();

        let root = resolve(user_path)?;
        if !root.exists() {
            anyhow::bail!("Path not found: {user_path}");
        }

        let mut matches: Vec<String> = Vec::new();
        let mut output_bytes: usize = 0;
        let mut truncated = false;
        search_path(&root, &re, &glob_pattern, &mut matches, max_results, &mut output_bytes, &mut truncated)?;

        if matches.is_empty() {
            return Ok(format!("No matches for {:?} in {user_path}.", pattern));
        }

        let mut out = format!("{} match(es):\n", matches.len());
        out.push_str(&matches.join("\n"));
        if truncated {
            out.push_str(&format!(
                "\n\n[Output truncated at {MAX_OUTPUT_BYTES} bytes. Narrow your search with a more specific pattern, path, or include_glob.]"
            ));
        }
        Ok(out)
    }
}

const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".venv", "__pycache__"];
const MAX_FILE_BYTES: u64 = 200_000;   // skip files larger than 200 KB
const MAX_OUTPUT_BYTES: usize = 60_000; // truncate total output at ~60 KB
const MAX_LINE_BYTES: usize = 500;      // truncate individual matching lines

fn search_path(
    path:         &std::path::Path,
    re:           &Regex,
    glob:         &Option<glob::Pattern>,
    matches:      &mut Vec<String>,
    max_results:  usize,
    output_bytes: &mut usize,
    truncated:    &mut bool,
) -> Result<()> {
    if matches.len() >= max_results || *truncated {
        return Ok(());
    }

    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if matches.len() >= max_results || *truncated { break; }
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if SKIP_DIRS.contains(&name) { continue; }
                search_path(&p, re, glob, matches, max_results, output_bytes, truncated)?;
            } else {
                search_file(&p, re, glob, matches, max_results, output_bytes, truncated)?;
            }
        }
    } else {
        search_file(path, re, glob, matches, max_results, output_bytes, truncated)?;
    }

    Ok(())
}

fn search_file(
    path:         &std::path::Path,
    re:           &Regex,
    glob:         &Option<glob::Pattern>,
    matches:      &mut Vec<String>,
    max_results:  usize,
    output_bytes: &mut usize,
    truncated:    &mut bool,
) -> Result<()> {
    if let Some(pat) = glob {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !pat.matches(name) { return Ok(()); }
    }

    // Skip files larger than MAX_FILE_BYTES.
    if let Ok(meta) = path.metadata() {
        if meta.len() > MAX_FILE_BYTES { return Ok(()); }
    }

    let text = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };

    // Skip binary files.
    if text.iter().take(8000).any(|&b| b == 0) { return Ok(()); }

    let content = match std::str::from_utf8(&text) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let display = path.to_string_lossy();

    for (i, line) in content.lines().enumerate() {
        if matches.len() >= max_results || *truncated { break; }
        if re.is_match(line) {
            let line_snippet = if line.len() > MAX_LINE_BYTES {
                format!("{}…", &line[..MAX_LINE_BYTES])
            } else {
                line.to_string()
            };
            let entry = format!("{}:{}: {}", display, i + 1, line_snippet);
            *output_bytes += entry.len();
            if *output_bytes > MAX_OUTPUT_BYTES {
                *truncated = true;
                break;
            }
            matches.push(entry);
        }
    }

    Ok(())
}
