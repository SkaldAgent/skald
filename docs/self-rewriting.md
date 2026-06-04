# Self-Rewriting

## Restart Mechanism

The `restart` tool calls `std::process::exit(-1)`. On Unix, Rust maps `-1` to exit code `255`. `run.sh` detects `255` and re-executes `cargo run`, which recompiles any changed source files and relaunches the app. All persistent state (sessions, cron jobs, LLM config) survives in SQLite.

---

## run.sh Exit Codes

| Exit code | Meaning | run.sh action |
|---|---|---|
| `0` | Clean shutdown (Ctrl+C) | Stop loop, exit 0 |
| `255` | Restart requested (`exit(-1)`) | `cargo run` again (recompile) |
| other | Unexpected error | Stop loop, propagate code |

---

## Safe Self-Modification Workflow

1. **Read** the relevant source files with `read_file` before making any changes.
2. **Edit** source files (`edit_file`, `write_file`, etc.).
3. **Check**: `execute_cmd` with command `cargo check 2>&1`. Inspect output.
4. **Fix** any compiler errors. Repeat steps 2–3 until clean.
5. **Restart**: call the `restart` tool only after a clean `cargo check`. The app rebuilds and relaunches automatically.

Never skip the `cargo check` step. A broken build will crash the supervisor loop with a non-zero non-255 exit code, stopping the app entirely.

---

## Requires Restart vs Does Not

| Change | Restart required? |
|---|---|
| `src/**/*.rs` | **Yes** |
| `Cargo.toml` / `Cargo.lock` | **Yes** |
| `agents/*/AGENT.md` | No — read at request time |
| `agents/*/meta.json` | No — read at request time |
| `config.yml` | No — read at startup only; take effect on next restart |
| `data/memory/**` | No — read at request time |
| `docs/**` | No |

---

## Risk Points

- **Never call `restart` mid-approval flow.** If a `PendingWrite` is waiting for user input, calling `restart` drops the `oneshot` sender, which unblocks the handler with an `Err` — the approval is cancelled and the tool call is aborted. Wait for the approval to resolve first.
- **Always check build before restart.** A compilation failure with `cargo run` returns a non-255 exit code, causing `run.sh` to stop the loop rather than retry.
- **`execute_cmd` requires user approval.** The user must approve the shell command in the UI before it executes.

---

## When to Update This File

- The restart mechanism or exit codes change
- The safe-modification workflow gains or loses a step
- New file types are added that do/don't require a restart
