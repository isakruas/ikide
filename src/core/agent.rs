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

//! Drives an installed coding-agent CLI (`claude`, `codex`, or a user-defined
//! custom command) as a subprocess, wired to the bundled `ikmcp` tools.
//!
//! Crucially this needs **no API key**: each CLI runs with the user's own
//! subscription login. The agent reaches the ik compiler, simulator and IDE
//! tools because we re-exec this very binary as `ikide mcp` (a stdio MCP server)
//! and register it with the CLI:
//!   * Claude:  `--mcp-config '{"mcpServers":{"ikmcp":{...}}}'`
//!   * Codex:   `-c mcp_servers.ikmcp.command=...`
//!   * Custom:  user's own command (`{prompt}` placeholder; no MCP wiring)
//!
//! Output is streamed back over an [`AgentMsg`] channel: tool calls/responses as
//! they happen (for the panel's activity log) plus a final assistant reply.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub enum AgentMsg {
    Progress(String),
    ToolCall { name: String, args: String },
    ToolResponse { name: String, response: String },
    Response(String),
    Error(String),
    /// The agent asked the IDE to perform an action in its own panels (compile /
    /// simulate / test) via the IDE bridge. The IDE runs it and writes the result
    /// back to `result_pipe`, keyed by `id`, which the agent's tool is awaiting.
    IdeCommand {
        id: String,
        action: String,
        file: Option<String>,
        payload: Option<String>,
        result_pipe: String,
    },
}

/// Base system guidance prepended to every agent run: steer it to drive the IDE
/// (ide_compile / ide_simulate) instead of dumping artifacts into the chat.
/// [`build_guidance`] wraps this with the user's preset context and language.
const IDE_GUIDANCE_BASE: &str = "You are the coding assistant embedded in the IKIDE IDE for the ik language \
(ik8b / AVR-8). ik is a small, strongly typed, bare-metal language for 8-bit AVR microcontrollers: no heap, \
no runtime, compiling straight to Intel HEX. \
To build or compile, call the `ide_compile` tool; to run or simulate, call `ide_simulate`; \
to run the test suite, call `ide_test`. Those run inside the IDE and show their output in its Output and \
Simulation panels — do NOT use ik_compile, ik_simulate or ide_run_tests, and never paste raw Intel HEX into \
the chat. To set up the breadboard/circuit for the user's program, call `ide_breadboard_catalog` to see the \
available devices and target pins, then `ide_breadboard_set` to place and wire them. \
For special cases the ide_* tools don't cover (e.g. capturing a full instruction trace, custom flags), \
you can run the IDE's own toolchain directly via the shell using the $IKIDE_BIN executable — the same \
compiler/simulator the IDE uses. Examples: `\"$IKIDE_BIN\" build file.ik -o out.hex`, \
`\"$IKIDE_BIN\" sim out.hex --mcu atmega328p --trace --limit 200000`, `\"$IKIDE_BIN\" run file.ik --dump`. \
Run `\"$IKIDE_BIN\" help` to see every subcommand and flag. Prefer the ide_* tools for normal builds/runs \
(they show in the IDE); use $IKIDE_BIN only when you need a capability they don't expose. \
When unsure about ik syntax, the standard library or a device model, consult the ikmcp knowledge tools \
(ik_* resources) rather than guessing — they read the pinned, always-accurate reference. \
Edit the user's .ik files directly to make changes. \
Follow the project's file layout when creating files, and never scatter them elsewhere: \
the entry source is `main.ik` with any extra `.ik` modules beside it at the workspace root; \
tests go in `tests/*.rhai`; custom virtual devices/peripherals go in `devices/*.rhai` (then they appear \
in the breadboard catalog); the compiled `.hex` is written to `build/` by ide_compile — it is a generated \
artifact, so never hand-write or edit it. Set up the breadboard with the `ide_breadboard_catalog` / \
`ide_breadboard_set` tools rather than writing board files by hand; an exported board is a `.json` \
(e.g. `breadboard.json`). \
Reply in PLAIN TEXT only, suitable for a narrow side panel: no Markdown at all — no **bold**, no #headings, \
no `backticks` or code fences, no tables, no bullet/list markup. Keep replies short and focused on what you \
did and why.";

/// Build the full hidden system guidance for one turn: the base IDE steering,
/// plus the user's preset project context and preferred reply language. Both
/// extras are optional and trimmed; empty/"auto" values are skipped.
fn build_guidance(language: &str, extra_context: &str) -> String {
    let mut out = String::from(IDE_GUIDANCE_BASE);
    let lang = language.trim();
    if !lang.is_empty() && !lang.eq_ignore_ascii_case("auto") {
        out.push_str(&format!(
            " Always write your replies to the user in {}, regardless of the language of their message \
(keep code, identifiers and tool arguments unchanged).",
            lang
        ));
    }
    let ctx = extra_context.trim();
    if !ctx.is_empty() {
        out.push_str("\n\n--- Project context provided by the user (treat as background, always relevant) ---\n");
        out.push_str(ctx);
    }
    out
}

/// Absolute path to the running ikide executable, used as the MCP server
/// command (`ikide mcp`). Falls back to the bare name so a PATH install works.
fn ikide_exe() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ikide".to_string())
}

/// The inline MCP-server config shared by the CLIs that accept one (Claude,
/// Codex). `ikide mcp` speaks MCP over stdio.
fn mcp_config_json(exe: &str) -> String {
    serde_json::json!({
        "mcpServers": {
            "ikmcp": { "command": exe, "args": ["mcp"] }
        }
    })
    .to_string()
}

/// Read any new lines appended to the bridge command file and forward each as an
/// [`AgentMsg::IdeCommand`]. `processed` tracks how many lines were already sent.
fn drain_ide_commands(path: &Path, processed: &mut usize, tx: &Sender<AgentMsg>, result_pipe: &str) {
    if let Ok(content) = std::fs::read_to_string(path) {
        let lines: Vec<&str> = content.lines().collect();
        while *processed < lines.len() {
            let line = lines[*processed];
            *processed += 1;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
                    if !action.is_empty() {
                        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let file = v.get("file").and_then(|f| f.as_str()).map(|s| s.to_string());
                        let payload = v.get("payload").and_then(|p| p.as_str()).map(|s| s.to_string());
                        let _ = tx.send(AgentMsg::IdeCommand {
                            id,
                            action: action.to_string(),
                            file,
                            payload,
                            result_pipe: result_pipe.to_string(),
                        });
                    }
                }
            }
        }
    }
}

/// Everything the agent run needs beyond the prompt: which CLI, its autonomy,
/// reply language, preloaded context and the editable command presets. Bundled
/// so the runner's signature stays small as options grow.
pub struct AgentRunConfig {
    pub kind: String,
    pub autonomy: String,
    pub language: String,
    pub extra_context: String,
    pub custom_command: String,
    pub claude_command: String,
    pub codex_command: String,
}

/// Run one turn of the selected agent CLI against `prompt`.
///
/// `cfg.kind` is `"claude"`, `"codex"` or `"custom"`; `cfg.autonomy` is
/// `"edits"` (auto-accept edits), `"yolo"` (auto-accept everything) or
/// `"readonly"`. Each invocation is independent — the CLIs are stateless here.
pub fn run_cli_agent(
    workspace_dir: Option<PathBuf>,
    cfg: AgentRunConfig,
    prompt: String,
    active_file: Option<PathBuf>,
    tx: Sender<AgentMsg>,
) {
    let AgentRunConfig {
        kind: agent_kind,
        autonomy,
        language,
        extra_context,
        custom_command,
        claude_command,
        codex_command,
    } = cfg;
    let workspace = workspace_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let exe = ikide_exe();
    let guidance = build_guidance(&language, &extra_context);

    let built = match agent_kind.as_str() {
        "claude" => build_templated(&claude_command, &prompt, &guidance, &exe, claude_autonomy(&autonomy)),
        "codex" => build_templated(&codex_command, &prompt, &guidance, &exe, codex_autonomy(&autonomy)),
        "custom" => build_custom(&custom_command, &prompt, &guidance),
        other => Err(format!("Unknown agent '{}'. Pick claude, codex or custom.", other)),
    };
    let mut cmd = match built {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(AgentMsg::Error(e));
            return;
        }
    };

    cmd.current_dir(&workspace);
    // Point the MCP server (a grandchild the CLI spawns) at this very binary for
    // the whole toolchain. ik8b/ik8bvm are embedded in ikide and exposed as the
    // `ikide build|run|sim|devices` subcommands, so the server compiles and
    // simulates through ikide alone — no separate ik8b binary to locate:
    //   * IK8B_BIN  — ikide itself (ikmcp calls it as `ikide build|run|sim|devices`)
    //   * IKIDE_BIN — ikide itself (ikmcp calls it as `ikide test`)
    //   * IK8B_ROOT / IKIDE_ROOT — work dir for relative paths and scratch files
    // Without these the server resolves nothing from a bare workspace cwd and
    // every build/simulate tool fails with "ik8b binary not found".
    let ws_str = workspace.to_string_lossy().to_string();
    cmd.env("IK8B_BIN", &exe);
    cmd.env("IKIDE_BIN", &exe);
    cmd.env("IK8B_ROOT", &ws_str);
    cmd.env("IKIDE_ROOT", &ws_str);

    // IDE bridge: a per-run command file the agent's ide_* tools append to (the
    // IDE tails it to compile/simulate/test in its panels) and a result file the
    // IDE writes outcomes back to, which those tools await (see crate::bridge).
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let cmd_pipe = std::env::temp_dir().join(format!("ikide_cmd_{}_{}.jsonl", std::process::id(), stamp));
    let result_pipe = std::env::temp_dir().join(format!("ikide_res_{}_{}.jsonl", std::process::id(), stamp));
    let _ = std::fs::write(&cmd_pipe, "");
    let _ = std::fs::write(&result_pipe, "");
    cmd.env(crate::bridge::ENV_CMD_PIPE, &cmd_pipe);
    cmd.env(crate::bridge::ENV_RESULT_PIPE, &result_pipe);
    if let Some(af) = &active_file {
        cmd.env(crate::bridge::ENV_ACTIVE_FILE, af);
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let _ = tx.send(AgentMsg::Progress(format!("Launching {}...", agent_kind)));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let hint = if e.kind() == std::io::ErrorKind::NotFound {
                format!("'{}' CLI not found on PATH. Install it and sign in once.", agent_kind)
            } else {
                format!("Failed to launch {}: {}", agent_kind, e)
            };
            let _ = tx.send(AgentMsg::Error(hint));
            return;
        }
    };

    // Drain stderr on a side thread so the pipe never blocks; surface its tail
    // for diagnostics if the run fails. MCP readiness logs land here too.
    let err_tx = tx.clone();
    let stderr = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut tail = String::new();
        if let Some(se) = stderr {
            for line in BufReader::new(se).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let _ = err_tx.send(AgentMsg::Progress(first_line(&line)));
                tail.push_str(&line);
                tail.push('\n');
                if tail.len() > 4000 {
                    tail = tail[tail.len() - 2000..].to_string();
                }
            }
        }
        tail
    });

    // Tail the bridge command file: forward IDE actions the agent enqueues
    // (compile/simulate) to the GUI as they happen, until the child exits.
    let done = Arc::new(AtomicBool::new(false));
    let tail_done = done.clone();
    let tail_tx = tx.clone();
    let tail_path = cmd_pipe.clone();
    let tail_result = result_pipe.to_string_lossy().into_owned();
    let tail_handle = std::thread::spawn(move || {
        let mut processed = 0usize;
        loop {
            drain_ide_commands(&tail_path, &mut processed, &tail_tx, &tail_result);
            if tail_done.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        // Final drain — runs after the child has fully exited, so it catches any
        // command written just before exit.
        drain_ide_commands(&tail_path, &mut processed, &tail_tx, &tail_result);
    });

    // Parse stdout per agent, accumulating the final assistant reply.
    let mut final_text = String::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            match agent_kind.as_str() {
                "claude" => handle_claude_line(&line, &tx, &mut final_text),
                "codex" => handle_codex_line(&line, &tx, &mut final_text),
                _ => handle_text_line(&line, &tx, &mut final_text),
            }
        }
    }

    let status = child.wait();
    done.store(true, Ordering::Relaxed);
    let _ = tail_handle.join();
    let _ = std::fs::remove_file(&cmd_pipe);
    let _ = std::fs::remove_file(&result_pipe);
    let stderr_tail = stderr_handle.join().unwrap_or_default();
    let final_text = final_text.trim().to_string();

    match status {
        Ok(s) if s.success() => {
            if final_text.is_empty() {
                let _ = tx.send(AgentMsg::Response(
                    "(agent finished without a textual reply — check the activity log above)"
                        .to_string(),
                ));
            } else {
                let _ = tx.send(AgentMsg::Response(final_text));
            }
        }
        Ok(s) => {
            if !final_text.is_empty() {
                let _ = tx.send(AgentMsg::Response(final_text));
            } else {
                let tail = stderr_tail.trim();
                let _ = tx.send(AgentMsg::Error(format!(
                    "{} exited with {}.\n{}",
                    agent_kind,
                    s,
                    if tail.is_empty() { "(no output)" } else { tail }
                )));
            }
        }
        Err(e) => {
            let _ = tx.send(AgentMsg::Error(format!("Failed waiting for {}: {}", agent_kind, e)));
        }
    }
}

// --- Per-agent command construction ----------------------------------------

/// Claude's autonomy → permission-mode flags, substituted for `{autonomy}`.
fn claude_autonomy(autonomy: &str) -> &'static str {
    match autonomy {
        "yolo" => "--permission-mode bypassPermissions",
        "readonly" => "--permission-mode plan",
        _ => "--permission-mode acceptEdits",
    }
}

/// Codex's autonomy → sandbox flags, substituted for `{autonomy}`. Codex exec
/// is non-interactive (no approval prompts), and the `-a/--ask-for-approval`
/// flag was removed in recent versions — passing it makes `codex exec` abort.
fn codex_autonomy(autonomy: &str) -> &'static str {
    match autonomy {
        "yolo" => "--dangerously-bypass-approvals-and-sandbox",
        "readonly" => "-s read-only",
        _ => "-s workspace-write",
    }
}

/// Build a command from an editable preset template (the claude/codex presets).
/// The template is whitespace-split into tokens; the first token is the program.
/// Standalone placeholder tokens expand to a single argument each — `{prompt}`,
/// `{guidance}`, `{full_prompt}` (guidance + message folded for Codex), `{mcp}`
/// (the inline MCP-server JSON) — while `{autonomy}` expands to zero or more
/// flag arguments. Any other token has `{exe}` substituted in place.
fn build_templated(
    template: &str,
    prompt: &str,
    guidance: &str,
    exe: &str,
    autonomy_flags: &str,
) -> Result<Command, String> {
    let full = format!("{}\n\n---\n\n{}", guidance, prompt);
    let mcp = mcp_config_json(exe);
    let mut args: Vec<String> = Vec::new();
    for token in template.split_whitespace() {
        match token {
            "{prompt}" => args.push(prompt.to_string()),
            "{guidance}" => args.push(guidance.to_string()),
            "{full_prompt}" => args.push(full.clone()),
            "{mcp}" => args.push(mcp.clone()),
            "{autonomy}" => args.extend(autonomy_flags.split_whitespace().map(String::from)),
            other => args.push(other.replace("{exe}", exe)),
        }
    }
    let Some((program, rest)) = args.split_first() else {
        return Err("Agent command preset is empty. Set it in Preferences → AI Assistant.".to_string());
    };
    let mut cmd = Command::new(program);
    cmd.args(rest);
    Ok(cmd)
}

/// User-defined agent. `command` is a whitespace-split command line; the first
/// token is the executable, the rest are arguments. The token `{prompt}` (in any
/// argument) is replaced with the guidance + user message; if no token is
/// present, that combined text is appended as a final argument. Output is read
/// as plain text — MCP/ide_* wiring is not assumed for arbitrary CLIs.
fn build_custom(command: &str, prompt: &str, guidance: &str) -> Result<Command, String> {
    let full = format!("{}\n\n---\n\n{}", guidance, prompt);
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let Some((program, rest)) = tokens.split_first() else {
        return Err(
            "Custom agent command is empty. Set it in Preferences → AI Assistant.".to_string(),
        );
    };
    let mut cmd = Command::new(program);
    let mut substituted = false;
    for arg in rest {
        if arg.contains("{prompt}") {
            cmd.arg(arg.replace("{prompt}", &full));
            substituted = true;
        } else {
            cmd.arg(arg);
        }
    }
    if !substituted {
        cmd.arg(&full);
    }
    Ok(cmd)
}

// --- Per-agent stdout parsing ----------------------------------------------

/// Plain-text streaming (custom agents, and a safe fallback): every non-empty line is
/// both a progress tick and part of the final reply.
fn handle_text_line(line: &str, tx: &Sender<AgentMsg>, final_text: &mut String) {
    if line.trim().is_empty() {
        return;
    }
    let _ = tx.send(AgentMsg::Progress(first_line(line)));
    final_text.push_str(line);
    final_text.push('\n');
}

/// Claude `--output-format stream-json`: one JSON object per line.
fn handle_claude_line(line: &str, tx: &Sender<AgentMsg>, final_text: &mut String) {
    let v: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return, // ignore non-JSON noise
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            if let Some(arr) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in arr {
                    match item.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                                if !t.trim().is_empty() {
                                    let _ = tx.send(AgentMsg::Progress(format!("💭 {}", first_line(t))));
                                }
                            }
                        }
                        Some("tool_use") => {
                            let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("tool");
                            let args = item
                                .get("input")
                                .map(|i| i.to_string())
                                .unwrap_or_default();
                            let _ = tx.send(AgentMsg::ToolCall {
                                name: name.to_string(),
                                args,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        Some("user") => {
            if let Some(arr) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in arr {
                    if item.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        let _ = tx.send(AgentMsg::ToolResponse {
                            name: "tool".to_string(),
                            response: extract_claude_tool_result(item),
                        });
                    }
                }
            }
        }
        Some("result") => {
            if let Some(r) = v.get("result").and_then(|x| x.as_str()) {
                *final_text = r.to_string();
            }
        }
        _ => {}
    }
}

fn extract_claude_tool_result(item: &serde_json::Value) -> String {
    match item.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let mut s = String::new();
            for c in arr {
                if let Some(t) = c.get("text").and_then(|x| x.as_str()) {
                    s.push_str(t);
                }
            }
            s
        }
        _ => String::new(),
    }
}

/// Codex `exec --json`: JSONL events. Tolerates both the `{"item":{...}}` and
/// the older `{"msg":{...}}` schemas; the last `agent_message` wins as the reply.
fn handle_codex_line(line: &str, tx: &Sender<AgentMsg>, final_text: &mut String) {
    let v: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return,
    };

    if let Some(item) = v.get("item") {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("agent_message") => {
                if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                    *final_text = t.to_string();
                }
            }
            Some(other) if other.contains("tool") || other.contains("command") => {
                let name = item
                    .get("tool")
                    .and_then(|x| x.as_str())
                    .or_else(|| item.get("name").and_then(|x| x.as_str()))
                    .unwrap_or(other);
                let _ = tx.send(AgentMsg::ToolCall {
                    name: name.to_string(),
                    args: String::new(),
                });
            }
            _ => {}
        }
        return;
    }

    if let Some(msg) = v.get("msg") {
        match msg.get("type").and_then(|t| t.as_str()) {
            Some("agent_message") => {
                if let Some(t) = msg.get("message").and_then(|x| x.as_str()) {
                    *final_text = t.to_string();
                }
            }
            Some("agent_reasoning") => {
                if let Some(t) = msg.get("text").and_then(|x| x.as_str()) {
                    let _ = tx.send(AgentMsg::Progress(format!("💭 {}", first_line(t))));
                }
            }
            Some(t)
                if t.contains("tool_call") || t.starts_with("mcp_tool") || t.starts_with("exec_command") =>
            {
                let name = msg
                    .get("tool")
                    .and_then(|x| x.as_str())
                    .or_else(|| msg.get("invocation").and_then(|x| x.as_str()))
                    .or_else(|| msg.get("command").and_then(|x| x.as_str()))
                    .unwrap_or(t);
                let _ = tx.send(AgentMsg::ToolCall {
                    name: name.to_string(),
                    args: String::new(),
                });
            }
            _ => {}
        }
    }
}

/// First line of `s`, clipped, for compact status/progress lines.
fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(140).collect()
}

/// Open the user's chosen agent CLI in a real terminal emulator, in the
/// workspace directory — for an interactive session outside the chat panel.
pub fn launch_agent_terminal(workspace_dir: Option<PathBuf>, command_str: &str) -> Result<(), String> {
    let workspace = workspace_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Support common terminal emulators on Linux.
    let term_emulators = [
        ("gnome-terminal", vec!["--"]),
        ("kitty", vec!["--"]),
        ("alacritty", vec!["-e"]),
        ("xterm", vec!["-e"]),
        ("konsole", vec!["-e"]),
    ];

    for (term, args) in &term_emulators {
        let mut cmd = std::process::Command::new(term);
        cmd.current_dir(&workspace);
        for arg in args {
            cmd.arg(arg);
        }

        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Command is empty".to_string());
        }
        for part in parts {
            cmd.arg(part);
        }

        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(e) => {
                return Err(format!("Error spawning terminal emulator {}: {}", term, e));
            }
        }
    }

    Err("No supported terminal emulator found (tried gnome-terminal, kitty, alacritty, xterm, konsole)".to_string())
}
