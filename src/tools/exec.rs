use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::tools::{Tool, ToolDescriptionLength, truncate_label, MAX_LABEL_SHORT, MAX_LABEL_FULL};

const TIMEOUT_SECS: u64 = 120;

pub struct ExecuteCmd;

impl Tool for ExecuteCmd {
    fn name(&self) -> &str { crate::tools::tool_names::EXECUTE_CMD }
    fn category(&self) -> crate::tools::ToolCategory { crate::tools::ToolCategory::Shell }

    fn description(&self) -> &str {
        "Execute a shell command (interpreted by `sh -c`) from the project root. \
         Captures stdout, stderr, and exit status. \
         Times out after 120 seconds. \
         Requires user approval before running."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type":        "string",
                    "description": "Full command line, passed to `sh -c`. May include pipes, redirects, and shell expansions."
                }
            },
            "required": ["command"]
        })
    }

    fn describe(&self, args: &Value, length: ToolDescriptionLength) -> String {
        let cmd = args["command"].as_str().unwrap_or("?");
        match length {
            ToolDescriptionLength::Short => {
                let binary = cmd.split_whitespace().next().unwrap_or(cmd);
                let name = binary.split('/').last().unwrap_or(binary);
                truncate_label(&format!("execute_cmd `{name}`"), MAX_LABEL_SHORT)
            }
            ToolDescriptionLength::Full => {
                truncate_label(&format!("execute_cmd `{cmd}`"), MAX_LABEL_FULL)
            }
        }
    }

    fn execute(&self, args: Value) -> Result<String> {
        let command = args["command"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: command"))?
            .to_string();

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(run(command))
        })
    }
}

async fn run(command: String) -> Result<String> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let status = match tokio::time::timeout(
        Duration::from_secs(TIMEOUT_SECS),
        child.wait(),
    ).await {
        Ok(s) => s?,
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            anyhow::bail!("Command timed out after {TIMEOUT_SECS} seconds: {command}");
        }
    };

    let mut out = String::new();
    if let Some(s) = stdout.as_mut() { s.read_to_string(&mut out).await?; }
    let mut err = String::new();
    if let Some(s) = stderr.as_mut() { s.read_to_string(&mut err).await?; }

    let code = status.code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "signal".to_string());

    Ok(format!(
        "exit: {code}\n--- stdout ---\n{out}\n--- stderr ---\n{err}"
    ))
}
