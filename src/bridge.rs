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
fn send_command(action: &str, file: Option<String>, payload: Option<String>) -> Result<String, String> {
    let pipe = std::env::var(ENV_CMD_PIPE)
        .map_err(|_| "the IDE bridge is unavailable (run this from the IKIDE AI chat)".to_string())?;
    let id = next_id();
    let line = serde_json::json!({ "id": id, "action": action, "file": file, "payload": payload });
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
    let id = send_command(action, file, None)?;
    Ok(serde_json::Value::String(await_result(&id)))
}

/// Board pins available on the active file's target MCU (for wiring devices).
fn active_target_pins() -> (Option<String>, Vec<String>) {
    let file = match std::env::var(ENV_ACTIVE_FILE) {
        Ok(s) if !s.is_empty() => s,
        _ => return (None, Vec::new()),
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(_) => return (None, Vec::new()),
    };
    match crate::core::board::target_of(&src) {
        Some(dev) => {
            let pins = crate::core::board::pins(&dev).into_iter().map(|p| p.name).collect();
            (Some(dev), pins)
        }
        None => (None, Vec::new()),
    }
}

/// List the device catalog and the target's board pins, so the agent knows what
/// it can place and how to wire it. Read-only — answered in-process.
fn ide_breadboard_catalog(_paths: &Paths, _args: serde_json::Value) -> Result<serde_json::Value, String> {
    let catalog = crate::core::devices::catalog();
    let devices: Vec<serde_json::Value> = catalog
        .iter()
        .map(|s| {
            let terminals: Vec<serde_json::Value> = s
                .pins
                .iter()
                .map(|p| serde_json::json!({ "name": p.name, "mode": format!("{:?}", p.mode) }))
                .collect();
            serde_json::json!({
                "id": s.id,
                "name": s.name,
                "bus": format!("{:?}", s.bus),
                "terminals": terminals,
            })
        })
        .collect();
    let (target, board_pins) = active_target_pins();
    Ok(serde_json::json!({
        "target": target,
        "board_pins": board_pins,
        "devices": devices,
        "config_format": "Pass to ide_breadboard_set as {\"config\":{\"clock_hz\":16000000,\"spi_miso\":255,\"devices\":[{\"spec_id\":\"<id>\",\"pins\":{\"<terminal>\":\"<board_pin>\"},\"adc\":{\"<terminal>\":<ch>}}]}}",
    }))
}

fn ide_breadboard_get(_paths: &Paths, _args: serde_json::Value) -> Result<serde_json::Value, String> {
    let id = send_command("bb_get", None, None)?;
    Ok(serde_json::Value::String(await_result(&id)))
}

fn ide_breadboard_set(_paths: &Paths, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let cfg = args
        .get("config")
        .ok_or("missing `config` object (see ide_breadboard_catalog for the format)")?;
    let payload = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
    let id = send_command("bb_set", None, Some(payload))?;
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
    // Breadboard control: assemble the user's circuit for them.
    server.register_tool(
        "ide_breadboard_catalog",
        "List the breadboard device catalog and the target MCU's board pins, so you know which devices you can place (spec ids, terminals, modes) and which pins to wire them to. Call this first when assembling a breadboard.",
        serde_json::json!({ "type": "object", "properties": {} }),
        ide_breadboard_catalog,
    );
    server.register_tool(
        "ide_breadboard_get",
        "Return the IDE's current breadboard setup (placed devices + wiring + clock) as JSON.",
        serde_json::json!({ "type": "object", "properties": {} }),
        ide_breadboard_get,
    );
    server.register_tool(
        "ide_breadboard_set",
        "Assemble the breadboard in the RUNNING IKIDE: replace the placed devices with the given configuration (devices wired by board-pin name). Use ide_breadboard_catalog for valid spec ids, terminals and pins. This sets up the board so the user can just run it.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "description": "Breadboard config: {clock_hz, spi_miso, devices:[{spec_id, pins:{terminal:board_pin}, adc:{terminal:ch}}]}"
                }
            },
            "required": ["config"]
        }),
        ide_breadboard_set,
    );
}
