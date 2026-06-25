// Copyright 2026 The IKIDE Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! IDE bridge: extra MCP tools that let the AI agent drive the *running* IDE.
//!
//! These tools do **not** compile or simulate themselves — they ask the IDE to,
//! and return the IDE's own result. The work runs exactly once, in the IDE's
//! Output / Simulation panels (like clicking the buttons), and the concise
//! result (never raw HEX) flows back to the agent. A request/response loop over
//! two files makes this work across the process boundary:
//!
//!   * `IKIDE_CMD_PIPE`    — tools append `{id, action, file}` here; the IDE tails it.
//!   * `IKIDE_RESULT_PIPE` — the IDE appends `{id, text}` here when the action ends.
//!   * `IKIDE_ACTIVE_FILE` — the file open in the editor (default target).
//!
//! Registered on the `ikide mcp` server alongside the bundled ikmcp tools.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ikmcp::paths::Paths;
use ikmcp::protocol::Server;

pub const ENV_CMD_PIPE: &str = "IKIDE_CMD_PIPE";
pub const ENV_RESULT_PIPE: &str = "IKIDE_RESULT_PIPE";
pub const ENV_ACTIVE_FILE: &str = "IKIDE_ACTIVE_FILE";

/// How long a tool waits for the IDE to finish an action before giving up.
const RESULT_TIMEOUT: Duration = Duration::from_secs(180);

static REQ_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("{}-{}", std::process::id(), REQ_COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// The source file a tool call targets: an explicit `path` argument (relative to
/// the server's cwd, i.e. the workspace) or the IDE's currently open file.
fn target_file(args: &serde_json::Value) -> Option<String> {
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        if !p.trim().is_empty() {
            let pb = std::path::PathBuf::from(p);
            let abs = if pb.is_absolute() {
                pb
            } else {
                std::env::current_dir().unwrap_or_default().join(pb)
            };
            return Some(abs.to_string_lossy().into_owned());
        }
    }
    match std::env::var(ENV_ACTIVE_FILE) {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Append a command for the IDE to run, returning the request id to await.
fn send_command(action: &str, file: Option<String>) -> Result<String, String> {
    let pipe = std::env::var(ENV_CMD_PIPE)
        .map_err(|_| "the IDE bridge is unavailable (run this from the IKIDE AI chat)".to_string())?;
    let id = next_id();
    let line = serde_json::json!({ "id": id, "action": action, "file": file });
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&pipe)
        .map_err(|e| format!("bridge write failed: {}", e))?;
    writeln!(f, "{}", line).map_err(|e| format!("bridge write failed: {}", e))?;
    Ok(id)
}

/// Block until the IDE writes a result for `id` (or we time out), then return it.
fn await_result(id: &str) -> String {
    let pipe = match std::env::var(ENV_RESULT_PIPE) {
        Ok(p) => p,
        Err(_) => return "Action sent to the IDE.".to_string(),
    };
    let deadline = Instant::now() + RESULT_TIMEOUT;
    loop {
        if let Ok(content) = std::fs::read_to_string(&pipe) {
            for line in content.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    if v.get("id").and_then(|x| x.as_str()) == Some(id) {
                        return v
                            .get("text")
                            .and_then(|x| x.as_str())
                            .unwrap_or("(the IDE finished the action)")
                            .to_string();
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return "The IDE did not return a result in time (it may still be running).".to_string();
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

/// Ask the IDE to run `action` on `file` and return the IDE's own result.
fn drive_ide(action: &str, file: Option<String>) -> Result<serde_json::Value, String> {
    let id = send_command(action, file)?;
    Ok(serde_json::Value::String(await_result(&id)))
}

fn ide_compile(_paths: &Paths, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let file = target_file(&args);
    if file.is_none() {
        return Err("no target file: open a .ik file in the IDE or pass `path`".to_string());
    }
    drive_ide("compile", file)
}

fn ide_simulate(_paths: &Paths, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let file = target_file(&args);
    if file.is_none() {
        return Err("no target file: open a .ik file in the IDE or pass `path`".to_string());
    }
    drive_ide("simulate", file)
}

fn ide_test(_paths: &Paths, _args: serde_json::Value) -> Result<serde_json::Value, String> {
    drive_ide("test", None)
}

/// Register the IDE-bridge tools on a server. Called by `ikide mcp`.
pub fn register(server: &mut Server) {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Workspace-relative path to the .ik file. Omit to use the file currently open in the IDE."
            }
        }
    });
    server.register_tool(
        "ide_compile",
        "Compile a .ik file in the RUNNING IKIDE, exactly as clicking Compile does — the build runs in the IDE's Output panel and its result (pass/fail with diagnostics, never raw HEX) is returned to you. Prefer this over ik_compile when working inside the IDE.",
        schema.clone(),
        ide_compile,
    );
    server.register_tool(
        "ide_simulate",
        "Build and run a .ik file in the RUNNING IKIDE simulator, exactly as clicking Run Simulation does — it runs in the IDE's Simulation panel and the result is returned to you (never raw HEX). Prefer this over ik_simulate when working inside the IDE.",
        schema,
        ide_simulate,
    );
    server.register_tool(
        "ide_test",
        "Run the workspace's test suite (tests/*.rhai) in the RUNNING IKIDE, exactly as clicking Run Tests does — it runs in the IDE's Output panel and the PASS/FAIL report is returned to you. Prefer this over ide_run_tests when working inside the IDE.",
        serde_json::json!({ "type": "object", "properties": {} }),
        ide_test,
    );
}
