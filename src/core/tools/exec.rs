use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::core::tools::{Tool, ToolDescriptionLength, truncate_label, MAX_LABEL_SHORT, MAX_LABEL_FULL};

const TIMEOUT_SECS: u64 = 120;

pub struct ExecuteCmd;

impl Tool for ExecuteCmd {
    fn name(&self) -> &str { crate::core::tools::tool_names::EXECUTE_CMD }
    fn category(&self) -> crate::core::tools::ToolCategory { crate::core::tools::ToolCategory::Shell }

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
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    // Read stdout/stderr concurrently with wait() inside a single timeout.
    //
    // Bug prevented: reading *after* wait() deadlocks when the pipe buffer fills
    // (~64KB) because the child blocks writing while the parent blocks on wait().
    // Bug prevented: timeout must also cover the reads — background processes
    // spawned by the command can hold the pipe descriptors open indefinitely even
    // after the main shell exits, so unbounded reads outside the timeout block forever.
    let result = tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), async {
        let (out_res, err_res, status_res) = tokio::join!(
            async {
                let mut buf = String::new();
                tokio::io::BufReader::new(stdout).read_to_string(&mut buf).await?;
                Ok::<_, std::io::Error>(buf)
            },
            async {
                let mut buf = String::new();
                tokio::io::BufReader::new(stderr).read_to_string(&mut buf).await?;
                Ok::<_, std::io::Error>(buf)
            },
            child.wait(),
        );
        Ok::<_, anyhow::Error>((out_res?, err_res?, status_res?))
    })
    .await;

    match result {
        Ok(Ok((out, err, status))) => {
            let code = status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            Ok(format!("exit: {code}\n--- stdout ---\n{out}\n--- stderr ---\n{err}"))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            anyhow::bail!("Command timed out after {TIMEOUT_SECS} seconds: {command}");
        }
    }
}
