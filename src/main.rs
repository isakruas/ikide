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
    println!("  version                Print version");
    println!("  license                Print license information");
    println!("  help                   Show this help text");
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
