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

// Modules live in the library crate (src/lib.rs) so the IDE and external tools
// share one copy of the code. main.rs is just the binary entry point: with no
// command it launches the graphical IDE; with `test` it runs a workspace's Rhai
// test suite headless, mirroring the rest of the toolchain's CLI conventions.
use ikide::app::IkIdeApp;
use ikide::core::runner::TaskMsg;
use ikide::core::testbed::spawn_run_tests;
use std::io::Write;
use std::path::PathBuf;
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LICENSE: &str = "Apache-2.0";

fn print_help(program: &str) {
    println!("ikide {} - The official IDE and test runner for ik8b", VERSION);
    println!();
    println!("Usage: {} [command] [arguments]", program);
    println!();
    println!("With no command, ikide launches the graphical IDE.");
    println!();
    println!("Commands:");
    println!("  test [workspace]       Run the workspace's tests/*.rhai suite headless");
    println!("  build <file.ik>        Compile a source file to Intel HEX");
    println!("  run   <file.ik>        Compile, then simulate the result");
    println!("  sim   <file.hex>       Simulate an already-assembled HEX image (--mcu <dev>)");
    println!("  devices                List supported target devices");
    println!("  mcp                    Serve the bundled ikmcp tools over stdio (MCP server)");
    println!("  version                Print version");
    println!("  license                Print license information");
    println!("  help                   Show this help text");
    println!();
    println!("Build options (build, run):");
    println!("  -o <out.hex>           Output Intel HEX path (default: out.hex)");
    println!("  --emit <hex|ir>        Output kind: hex (default) or SSA ir");
    println!("  --report               Print a memory-usage report (SRAM_BYTES=<n> first)");
    println!();
    println!("Simulation options (run, sim):");
    println!("  --mcu <device>         Target device (sim only; run uses the source target)");
    println!("  --trace                Print an instruction trace");
    println!("  --dump                 Dump registers/PC/SP/SREG at exit");
    println!("  --limit <N>            Stop after N instructions");
    println!("  --irq <V>              Raise interrupt vector V at startup");
    println!("  --irq-at <V:STEP>      Raise vector V once at instruction STEP");
    println!("  --irq-every <V:N>      Raise vector V every N instructions");
    println!("  --peek <addr>          Print data memory at <addr> on exit (hex or decimal)");
    println!("  --peek-len <N>         Number of bytes for --peek (default: 1)");
    println!();
    println!("Test options:");
    println!("  --mcu <device>         Target device for prebuilt-image tests");
    println!("                         (default: atmega328p; build(...) tests infer it");
    println!("                         from the source's `target` declaration)");
    println!();
    println!("The test runner discovers `<workspace>/tests/*.rhai` (default workspace:");
    println!("the current directory), runs each against a fresh bench, prints a PASS/FAIL");
    println!("report, and exits non-zero on any failure. The IDE runs the same files via");
    println!("Run -> Run Tests.");
}

fn print_version() {
    println!("ikide {}", VERSION);
}

fn print_license() {
    println!("ikide {}", VERSION);
    println!("License: {}", LICENSE);
    println!("Copyright 2026 The IKIDE Authors");
    println!("Full text: LICENSE or https://www.apache.org/licenses/LICENSE-2.0");
}

/// Run the workspace's `tests/*.rhai` suite headless and exit with the verdict.
fn run_tests(rest: &[String], program: &str) -> ! {
    let mut workspace: Option<PathBuf> = None;
    let mut mcu = "atmega328p".to_string();

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--mcu" => {
                i += 1;
                match rest.get(i) {
                    Some(dev) => mcu = dev.clone(),
                    None => {
                        eprintln!("error: --mcu requires a device name");
                        process::exit(1);
                    }
                }
            }
            opt if opt.starts_with('-') => {
                eprintln!("Unknown test option: {}", opt);
                eprintln!("Run `{} help` for usage.", program);
                process::exit(1);
            }
            path => workspace = Some(PathBuf::from(path)),
        }
        i += 1;
    }

    let ws = workspace.or_else(|| std::env::current_dir().ok());

    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    spawn_run_tests(ws, mcu, tx, cancel);

    // Stream the report as it arrives and keep a copy for the exit verdict.
    let mut report = String::new();
    let stdout = std::io::stdout();
    while let Ok(msg) = rx.recv() {
        match msg {
            TaskMsg::Test(s) => {
                let mut h = stdout.lock();
                let _ = h.write_all(s.as_bytes());
                let _ = h.flush();
                report.push_str(&s);
            }
            TaskMsg::Done => break,
            _ => {}
        }
    }

    let failed = report.contains("  FAIL  ")
        || report.contains("script error")
        || !report.contains(", 0 failed,");
    process::exit(if failed { 1 } else { 0 });
}

/// Serve the bundled ikmcp tools over stdio (Model Context Protocol).
///
/// This is what the IDE's AI agents (the `claude` and `codex` CLIs)
/// spawn as their MCP server — re-executing this same binary as `ikide mcp` —
/// so they can drive the ik compiler, simulator and IDE tools using the user's
/// own CLI subscription, with no API key. It speaks JSON-RPC over stdin/stdout
/// and never opens a window.
fn run_mcp_server() -> ! {
    let paths = ikmcp::paths::resolve_paths();
    let mut server = ikmcp::protocol::Server::new("ikmcp", VERSION);
    ikmcp::lang::tools::register(&mut server);
    ikmcp::ide::tools::register(&mut server);
    ikmcp::prompts::register(&mut server);
    // Extra tools that drive the running IDE (compile/simulate in its panels),
    // available when launched from the IDE's AI chat.
    ikide::bridge::register(&mut server);
    server.serve_forever(&paths);
    process::exit(0);
}

/// Launch the graphical IDE.
fn run_gui() -> Result<(), eframe::Error> {
    let icon_data = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Failed to load icon");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_icon(std::sync::Arc::new(icon_data)),
        ..Default::default()
    };
    eframe::run_native(
        "IKIDE - The official IDE for ik8b",
        options,
        Box::new(|_cc| Ok(Box::<IkIdeApp>::default())),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let program = args.first().map(String::as_str).unwrap_or("ikide");

    match args.get(1).map(String::as_str) {
        // No command: launch the graphical IDE.
        None => {
            if let Err(e) = run_gui() {
                eprintln!("error: {}", e);
                process::exit(1);
            }
        }
        Some("test") => run_tests(&args[2..], program),
        Some("mcp") => run_mcp_server(),
        // Embedded ik8b/ik8bvm toolchain, exposed so the bundled MCP server and
        // its agents can compile and simulate through the ikide binary alone.
        Some("build") => ikide::cli::run_build(&args),
        Some("run") => ikide::cli::run_run(&args),
        Some("sim") | Some("sim-hex") => ikide::cli::run_sim(&args),
        Some("devices") | Some("list-devices") => ikide::cli::print_devices(),
        Some("-h") | Some("--help") | Some("help") => print_help(program),
        Some("-V") | Some("--version") | Some("version") => print_version(),
        Some("--license") | Some("license") => print_license(),
        Some(other) => {
            eprintln!("Unknown command or option: {}", other);
            eprintln!("Run `{} help` for usage.", program);
            process::exit(1);
        }
    }
}
