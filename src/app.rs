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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use eframe::egui;

use crate::core::analysis::{self, Diagnostic, SymbolIndex};
use crate::core::runner::{self, TaskMsg};
use crate::core::workspace::{self, FileNode};

/// How long the buffer must be idle before we re-run the background type-check.
const CHECK_DEBOUNCE: Duration = Duration::from_millis(500);
use crate::ui::{editor, explorer, right_panel, terminal};

/// Current local wall-clock time, formatted for an Output log prefix.
pub fn now_ts() -> String {
    chrono::Local::now().format("[%Y-%m-%d %H:%M:%S]").to_string()
}

/// Prefix a log chunk with the current timestamp (no-op for empty chunks, so
/// blank stderr fragments don't add noise).
fn stamp(msg: &str) -> String {
    if msg.trim().is_empty() {
        msg.to_string()
    } else {
        format!("{} {}", now_ts(), msg)
    }
}


/// Keep a peripheral transcript from growing without bound, trimming the oldest
/// text once it gets large.
fn cap_log(s: &mut String) {
    const MAX: usize = 64_000;
    if s.len() > MAX {
        *s = s[s.len() - MAX / 2..].to_string();
    }
}

/// True when `path` is an ik8b source the IDE should compile, lint and style.
pub fn is_ik_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("ik")
}

/// Rhai scripts (test scripts and device models) get Rhai syntax highlighting.
pub fn is_rhai_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("rhai")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialTab {
    Console,
    Plotter,
}

pub struct OpenTab {
    pub path: PathBuf,
    pub content: String,
    pub is_modified: bool,
    pub last_mtime: Option<std::time::SystemTime>,
    pub is_disk_different: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ExampleInfo {
    #[serde(alias = "name")]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(skip)]
    pub embedded_index: usize,
}

include!(concat!(env!("OUT_DIR"), "/examples_embed.rs"));

pub enum ActionPopup {
    None,
    Rename { path: PathBuf, new_name: String },
    CreateFile { parent_dir: PathBuf, new_name: String },
    CreateDir { parent_dir: PathBuf, new_name: String },
    Delete { path: PathBuf },
}

pub struct IkIdeApp {
    pub workspace_dir: Option<PathBuf>,
    pub workspace_tree: Option<FileNode>,
    
    // Tab management
    pub open_tabs: Vec<OpenTab>,
    pub active_tab: Option<usize>,
    
    pub terminal_output: String,
    pub vm_output: String,
    pub stats_data: Result<Option<crate::core::runner::StatsData>, String>,
    
    pub task_rx: Receiver<TaskMsg>,
    pub task_tx: Sender<TaskMsg>,
    
    pub dialog_rx: Option<Receiver<Option<PathBuf>>>,
    
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    
    pub show_terminal: bool,
    pub show_vm_trace: bool,
    pub show_stats: bool,
    pub show_minimap: bool,
    pub show_preferences: bool,
    pub show_about: bool,
    pub show_examples: bool,
    pub show_serial: bool,
    pub show_breadboard: bool,
    pub is_busy: bool,
    /// Cancellation flag for the in-flight task; the "Stop" button sets it so the
    /// worker's loops bail out. A fresh token is created per task so a cancelled
    /// run can never affect the next one.
    pub cancel: Arc<AtomicBool>,
    /// Where the active task streams its output, so a cancel notice lands in the
    /// right panel (true = VM/trace panel, false = terminal).
    pub busy_writes_vm: bool,

    // Breadboard: visual schematic driven by a live simulation.
    pub breadboard: crate::ui::breadboard::Breadboard,
    pub live: Option<crate::core::sim_live::LiveHandle>,
    pub live_regs: std::collections::HashMap<u32, u8>,
    pub live_cycles: u64,
    pub live_running: bool,
    pub live_status: String,
    pub live_sp: u16,
    pub live_sreg: u8,
    pub live_pc: u32,
    pub live_r: [u8; 32],
    pub live_sram_start: u32,
    pub live_sram_bytes: u32,
    pub show_vm_registers: bool,
    pub ram_history: Vec<u32>,
    pub live_sram_peak: u32,
    pub live_sram_static: u32,
    pub bb_clock_hz: u32,
    pub bb_tab: crate::ui::breadboard::BreadboardTab,
    // Serial-peripheral transcripts, fed from the live engine's captured events.
    pub uart_log: String,
    pub uart_send: String,
    /// Line ending appended by the UART tab's Send box.
    pub bb_uart_ending: crate::core::serial::LineEnding,
    pub spi_log: String,
    pub twi_log: String,
    pub bb_spi_miso: u8,
    /// Devices placed on the breadboard (wired instances of catalog entries).
    pub bb_instances: Vec<crate::ui::breadboard::Instance>,
    /// Cached device catalog (compiled once), for the add picker.
    pub device_catalog: Vec<crate::core::devices::DeviceSpec>,
    /// Latest device view state from the live engine: (instance, id) -> value.
    pub live_view: std::collections::HashMap<(usize, String), crate::core::devices::ViewVal>,
    /// UART tab: show the plotter instead of the text console.
    pub bb_uart_plot: bool,
    /// Accumulates the current UART line for the plotter parser.
    bb_uart_line: String,
    /// Display surfaces of attached devices, rendered in the breadboard.
    pub bb_displays: Vec<crate::core::devices::DisplayInfo>,
    /// Cached display textures keyed by device name (generation, handle).
    pub bb_textures: std::collections::HashMap<String, (u64, egui::TextureHandle)>,
    /// TWI decode state: the next data byte after a START is the address.
    twi_expect_addr: bool,

    // Serial monitor.
    pub serial: Option<crate::core::serial::SerialConn>,
    pub serial_port: String,
    pub serial_baud: u32,
    pub serial_output: String,
    pub serial_input: String,
    pub serial_ending: crate::core::serial::LineEnding,
    pub serial_tab: SerialTab,
    pub serial_line_buf: String,
    pub plot_history: Vec<Vec<f32>>,
    pub plot_labels: Vec<String>,
    pub plot_visible: Vec<bool>,
    pub plot_grid: bool,
    pub plot_center_line: bool,
    pub plot_window_size: usize,
    pub plot_show_stats: bool,
    pub examples: Vec<ExampleInfo>,
    pub example_search: String,
    pub example_page: usize,
    pub last_disk_check: Instant,
    
    // In-process simulation preferences.
    pub vm_max_cycles: u32,
    pub sim_trace: bool,
    pub sim_dump_regs: bool,
    pub sim_peek_addr: String,
    pub sim_peek_len: u32,
    /// Structured snapshot from the most recent simulation run.
    pub vm_result: Option<crate::core::runner::VmResult>,

    // Upload method: bootloader (default) vs avrdude.
    pub use_bootloader: bool,
    // Avrdude upload preferences
    pub avrdude_path: String,
    pub avrdude_programmer: String,
    pub avrdude_port: String,
    pub avrdude_baudrate: String,
    pub avrdude_additional_flags: String,
    pub avrdude_target: String,

    // Serial-bootloader upload preferences.
    pub bootloader_port: String,
    pub bootloader_baud: u32,

    // Burn Bootloader preferences.
    pub burn_path: String,
    pub burn_target: String,
    pub burn_f_cpu: u32,
    pub burn_baud: u32,
    pub burn_programmer: String,
    pub burn_port: String,
    pub burn_baudrate: String,
    pub burn_additional_flags: String,

    // Language intelligence (compiler-backed).
    pub std_index: SymbolIndex,
    pub devices: Vec<(String, String)>,
    pub diagnostics: Vec<Diagnostic>,
    /// A line the editor should scroll to on the next frame (1-based), set when
    /// the user clicks a problem so the cursor jumps to the offending code.
    pub pending_scroll_line: Option<usize>,
    pub last_edit: Option<Instant>,
    pub last_checked_content: String,

    pub action_popup: ActionPopup,
}

impl Default for IkIdeApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        // Restore persisted preferences / last project from the config file.
        let settings = crate::core::settings::Settings::load();
        // Index the std library straight from the sources baked into the binary
        // — every std symbol is known for completion without writing any file.
        // Modules are only materialized to disk on demand, when imported, by
        // `analysis::sync_std_imports` (kept out of the user's project).
        let std_index = analysis::std_symbol_index();
        // Supported target chips, straight from the compiler's device table.
        let devices = analysis::load_devices();

        // Reopen the last project if its folder still exists, making it the CWD
        // so local `import <module>` resolves against it.
        let workspace_dir = settings.last_workspace.clone().filter(|p| p.is_dir());
        if let Some(dir) = &workspace_dir {
            let _ = std::env::set_current_dir(dir);
        }
        let workspace_tree = workspace_dir.as_ref().map(|dir| {
            let mut tree = workspace::scan_workspace(dir);
            tree.name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            tree
        });

        let mut app = Self {
            workspace_dir,
            workspace_tree,
            open_tabs: Vec::new(),
            active_tab: None,
            terminal_output: "Welcome to IKIDE!\n".to_string(),
            vm_output: String::new(),
            stats_data: Ok(None),
            task_rx: rx,
            task_tx: tx,
            dialog_rx: None,
            left_panel_width: settings.left_panel_width,
            right_panel_width: settings.right_panel_width,
            show_terminal: settings.show_terminal,
            show_vm_trace: settings.show_vm_trace,
            show_stats: settings.show_stats,
            show_minimap: settings.show_minimap,
            show_preferences: false,
            show_about: false,
            show_examples: false,
            show_serial: false,
            show_breadboard: false,
            is_busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            busy_writes_vm: false,
            breadboard: crate::ui::breadboard::Breadboard::default(),
            live: None,
            live_regs: std::collections::HashMap::new(),
            live_cycles: 0,
            live_running: false,
            live_status: String::new(),
            live_sp: 0,
            live_sreg: 0,
            live_pc: 0,
            live_r: [0; 32],
            live_sram_start: 0,
            live_sram_bytes: 0,
            show_vm_registers: false,
            ram_history: Vec::new(),
            live_sram_peak: 0,
            live_sram_static: 0,
            bb_clock_hz: 16_000_000,
            bb_tab: crate::ui::breadboard::BreadboardTab::Schematic,
            uart_log: String::new(),
            uart_send: String::new(),
            bb_uart_ending: crate::core::serial::LineEnding::Lf,
            spi_log: String::new(),
            twi_log: String::new(),
            bb_spi_miso: 0xFF,
            bb_instances: Vec::new(),
            device_catalog: crate::core::devices::catalog(),
            live_view: std::collections::HashMap::new(),
            bb_uart_plot: false,
            bb_uart_line: String::new(),
            bb_displays: Vec::new(),
            bb_textures: std::collections::HashMap::new(),
            twi_expect_addr: false,
            serial: None,
            serial_port: settings.serial_port.clone(),
            serial_baud: settings.serial_baud,
            serial_output: String::new(),
            serial_input: String::new(),
            serial_ending: crate::core::serial::LineEnding::Lf,
            serial_tab: SerialTab::Console,
            serial_line_buf: String::new(),
            plot_history: Vec::new(),
            plot_labels: Vec::new(),
            plot_visible: Vec::new(),
            plot_grid: true,
            plot_center_line: true,
            plot_window_size: 500,
            plot_show_stats: true,
            examples: Vec::new(),
            example_search: String::new(),
            example_page: 0,
            last_disk_check: Instant::now(),
            vm_max_cycles: settings.vm_max_cycles,
            sim_trace: settings.sim_trace,
            sim_dump_regs: settings.sim_dump_regs,
            sim_peek_addr: settings.sim_peek_addr.clone(),
            sim_peek_len: settings.sim_peek_len,
            vm_result: None,
            use_bootloader: settings.use_bootloader,
            avrdude_path: settings.avrdude_path.clone(),
            avrdude_programmer: settings.avrdude_programmer.clone(),
            avrdude_port: settings.avrdude_port.clone(),
            avrdude_baudrate: settings.avrdude_baudrate.clone(),
            avrdude_additional_flags: settings.avrdude_additional_flags.clone(),
            avrdude_target: settings.avrdude_target.clone(),
            bootloader_port: settings.bootloader_port.clone(),
            bootloader_baud: settings.bootloader_baud,
            burn_path: settings.burn_path.clone(),
            burn_target: settings.burn_target.clone(),
            burn_f_cpu: settings.burn_f_cpu,
            burn_baud: settings.burn_baud,
            burn_programmer: settings.burn_programmer.clone(),
            burn_port: settings.burn_port.clone(),
            burn_baudrate: settings.burn_baudrate.clone(),
            burn_additional_flags: settings.burn_additional_flags.clone(),
            std_index,
            devices,
            diagnostics: Vec::new(),
            pending_scroll_line: None,
            last_edit: None,
            last_checked_content: String::new(),
            action_popup: ActionPopup::None,
        };

        app.scan_examples();
        app
    }
}

impl IkIdeApp {
    pub fn open_folder_dialog(&mut self) {
        if self.dialog_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.dialog_rx = Some(rx);
            std::thread::spawn(move || {
                let folder = rfd::FileDialog::new().pick_folder();
                let _ = tx.send(folder);
            });
        }
    }

    /// Snapshot the current preferences / layout / open project for saving.
    pub fn current_settings(&self) -> crate::core::settings::Settings {
        crate::core::settings::Settings {
            last_workspace: self.workspace_dir.clone(),
            vm_max_cycles: self.vm_max_cycles,
            sim_trace: self.sim_trace,
            sim_dump_regs: self.sim_dump_regs,
            sim_peek_addr: self.sim_peek_addr.clone(),
            sim_peek_len: self.sim_peek_len,
            use_bootloader: self.use_bootloader,
            avrdude_path: self.avrdude_path.clone(),
            avrdude_programmer: self.avrdude_programmer.clone(),
            avrdude_port: self.avrdude_port.clone(),
            avrdude_baudrate: self.avrdude_baudrate.clone(),
            avrdude_additional_flags: self.avrdude_additional_flags.clone(),
            avrdude_target: self.avrdude_target.clone(),
            bootloader_port: self.bootloader_port.clone(),
            bootloader_baud: self.bootloader_baud,
            burn_path: self.burn_path.clone(),
            burn_target: self.burn_target.clone(),
            burn_f_cpu: self.burn_f_cpu,
            burn_baud: self.burn_baud,
            burn_programmer: self.burn_programmer.clone(),
            burn_port: self.burn_port.clone(),
            burn_baudrate: self.burn_baudrate.clone(),
            burn_additional_flags: self.burn_additional_flags.clone(),
            serial_port: self.serial_port.clone(),
            serial_baud: self.serial_baud,
            left_panel_width: self.left_panel_width,
            right_panel_width: self.right_panel_width,
            show_terminal: self.show_terminal,
            show_vm_trace: self.show_vm_trace,
            show_stats: self.show_stats,
            show_minimap: self.show_minimap,
        }
    }

    /// Persist the current settings to the config file (best effort).
    pub fn persist(&self) {
        self.current_settings().save();
    }

    /// Drain whatever the serial reader thread produced into the monitor log.
    pub fn pump_serial(&mut self) {
        let mut msgs = Vec::new();
        if let Some(conn) = &self.serial {
            while let Ok(m) = conn.rx.try_recv() {
                msgs.push(m);
            }
        }
        let mut closed = false;
        for m in msgs {
            match m {
                crate::core::serial::SerialMsg::Data(s) => {
                    self.serial_output.push_str(&s);
                    if self.serial_output.len() > 100_000 {
                        self.serial_output = self.serial_output[self.serial_output.len() - 50_000..].to_string();
                    }
                    self.serial_line_buf.push_str(&s);
                    while let Some(pos) = self.serial_line_buf.find('\n') {
                        let line = self.serial_line_buf[..pos].trim().to_string();
                        self.ingest_plot_line(&line);
                        self.serial_line_buf = self.serial_line_buf[pos + 1..].to_string();
                    }
                }
                crate::core::serial::SerialMsg::Error(e) => {
                    self.serial_output.push_str(&format!("\n[serial error] {}\n", e));
                    closed = true;
                }
                crate::core::serial::SerialMsg::Closed => closed = true,
            }
        }
        if closed {
            self.serial = None;
        }
    }

    /// Parse one line of "label:value, value, ..." plot data and append it to
    /// the shared plot buffers. Used by both the serial monitor and the
    /// breadboard's UART plotter so they behave identically.
    pub fn ingest_plot_line(&mut self, line: &str) {
        let mut values: Vec<f32> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        for part in line.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty()) {
            if let Some(idx) = part.find(':') {
                let label = part[..idx].trim().to_string();
                if let Ok(val) = part[idx + 1..].trim().parse::<f32>() {
                    values.push(val);
                    labels.push(label);
                }
            } else if let Ok(val) = part.parse::<f32>() {
                values.push(val);
                labels.push(format!("Ch {}", values.len()));
            }
        }
        if values.is_empty() {
            return;
        }
        for (ch_idx, label) in labels.into_iter().enumerate() {
            if ch_idx >= self.plot_labels.len() {
                self.plot_labels.push(label);
                self.plot_visible.push(true);
            } else if label != format!("Ch {}", ch_idx + 1) {
                self.plot_labels[ch_idx] = label;
            }
        }
        self.plot_history.push(values);
        if self.plot_history.len() > 100_000 {
            self.plot_history.remove(0);
        }
    }

    /// Assemble the simulation knobs from the current preferences, parsing the
    /// optional memory-peek address (accepts `0x`-hex or decimal; blank = off).
    pub fn sim_config(&self) -> crate::core::runner::SimConfig {
        let peek_addr = {
            let s = self.sim_peek_addr.trim();
            if s.is_empty() {
                None
            } else if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                s.parse::<u32>().ok()
            }
        };
        crate::core::runner::SimConfig {
            max_instr: self.vm_max_cycles,
            trace: self.sim_trace,
            dump_regs: self.sim_dump_regs,
            peek_addr,
            peek_len: self.sim_peek_len.max(1),
        }
    }

    /// Compile the active buffer and start a live breadboard simulation: the
    /// VM runs continuously and the breadboard panel mirrors its I/O ports.
    pub fn start_breadboard(&mut self) {
        let content = match self.active_tab.and_then(|i| self.open_tabs.get(i)) {
            Some(t) => t.content.clone(),
            None => return,
        };
        self.stop_breadboard();
        self.live_status.clear();

        analysis::sync_std_imports(&content);
        let artifact = match analysis::compile(&content) {
            Ok(a) => a,
            Err(diag) => {
                let loc = if diag.line > 0 { format!("line {}: ", diag.line) } else { String::new() };
                self.live_status = format!("Compilation failed — {}{}", loc, diag.message);
                return;
            }
        };
        let device = artifact.device.clone();
        // Write the HEX to a temp file the live engine loads into the core.
        let hex_path = std::env::temp_dir().join("ikide_breadboard.hex");
        if let Err(e) = std::fs::write(&hex_path, &artifact.hex) {
            self.live_status = format!("Failed to stage HEX: {}", e);
            return;
        }

        let watch = crate::core::board::watch_addrs(&device);
        self.breadboard.ensure_pins(&device);
        self.live_regs.clear();
        self.live_cycles = 0;
        self.live_running = true;
        self.live_sp = 0;
        self.live_sreg = 0;
        self.live_pc = 0;
        self.live_r = [0; 32];
        self.live_sram_start = 0;
        self.live_sram_bytes = 0;
        self.ram_history.clear();
        self.live_sram_peak = 0;
        self.live_sram_static = artifact.sram_used;
        // Clearing the peripheral logs gives each run a fresh transcript.
        self.uart_log.clear();
        self.spi_log.clear();
        self.twi_log.clear();
        self.twi_expect_addr = false;
        // Fresh plot for the UART plotter tab.
        self.bb_uart_line.clear();
        self.plot_history.clear();
        self.plot_labels.clear();
        self.plot_visible.clear();
        // Instantiate the devices placed on the breadboard, with their wiring.
        let wirings: Vec<crate::core::devices::InstanceWiring> = self
            .bb_instances
            .iter()
            .map(|inst| inst.wiring(&self.device_catalog, self.breadboard.pins()))
            .collect();
        let bus = crate::core::devices::build_bus(&device, &wirings);
        self.live_view.clear();
        let bus = if bus.is_empty() {
            self.bb_displays = Vec::new();
            None
        } else {
            self.bb_displays = bus.displays();
            Some(std::sync::Arc::new(std::sync::Mutex::new(bus)))
        };

        self.live = Some(crate::core::sim_live::spawn(
            device,
            hex_path,
            self.bb_clock_hz,
            watch,
            self.bb_spi_miso,
            bus,
        ));
    }

    /// Tear down a running breadboard simulation.
    pub fn stop_breadboard(&mut self) {
        if let Some(live) = self.live.take() {
            live.stop();
        }
        self.live_running = false;
    }

    /// Drain the live engine's latest register snapshots into the UI state.
    /// Returns true while a live simulation is active (so the caller keeps
    /// repainting).
    pub fn pump_live(&mut self) -> bool {
        let mut halt: Option<String> = None;
        let mut events: Vec<crate::core::sim_live::IoEvent> = Vec::new();
        let mut got_snap = false;
        if let Some(live) = &self.live {
            while let Ok(snap) = live.snap_rx.try_recv() {
                self.live_regs = snap.regs;
                self.live_cycles = snap.cycles;
                self.live_running = snap.running;
                self.live_view = snap.view;
                self.live_sp = snap.sp;
                self.live_sreg = snap.sreg;
                self.live_pc = snap.pc;
                self.live_r = snap.r;
                self.live_sram_start = snap.sram_start;
                self.live_sram_bytes = snap.sram_bytes;
                events.extend(snap.events);
                if let Some(reason) = snap.halt_reason {
                    halt = Some(reason);
                }
                got_snap = true;
            }
        } else {
            return false;
        }
        for ev in events {
            self.route_io_event(ev);
        }
        if let Some(reason) = halt {
            self.live_status = reason;
        }
        if got_snap && self.live_running {
            let stack_bytes = (self.live_sram_start + self.live_sram_bytes)
                .saturating_sub(1)
                .saturating_sub(self.live_sp as u32);
            let total_used = self.live_sram_static + stack_bytes;
            self.ram_history.push(total_used);
            if total_used > self.live_sram_peak {
                self.live_sram_peak = total_used;
            }
            if self.ram_history.len() > 3600 {
                self.ram_history.remove(0);
            }
        }
        true
    }

    /// Append a captured serial-peripheral event to the matching transcript.
    fn route_io_event(&mut self, ev: crate::core::sim_live::IoEvent) {
        use ik8bvm::core::{IoKind, IoPeripheral};
        match ev.periph {
            IoPeripheral::Uart => {
                // UART carries text: keep printable bytes and newlines verbatim,
                // show other control bytes as a dot.
                let b = ev.byte;
                if b == b'\n' || b == b'\r' || b == b'\t' || (0x20..=0x7e).contains(&b) {
                    self.uart_log.push(b as char);
                } else {
                    self.uart_log.push('.');
                }
                cap_log(&mut self.uart_log);
                // Feed the UART plotter line-by-line (parity with the monitor).
                if b == b'\n' {
                    let line = std::mem::take(&mut self.bb_uart_line);
                    self.ingest_plot_line(line.trim());
                } else if b != b'\r' {
                    self.bb_uart_line.push(b as char);
                    if self.bb_uart_line.len() > 4096 {
                        self.bb_uart_line.clear();
                    }
                }
            }
            IoPeripheral::Spi => {
                // Full-duplex pairs: "MOSI→MISO ".
                if ev.write {
                    self.spi_log.push_str(&format!("{:02X}", ev.byte));
                } else {
                    self.spi_log.push_str(&format!("→{:02X} ", ev.byte));
                }
                cap_log(&mut self.spi_log);
            }
            IoPeripheral::Twi => {
                match ev.kind {
                    IoKind::TwiStart => {
                        self.twi_log.push_str("\n[S] ");
                        self.twi_expect_addr = true;
                    }
                    IoKind::TwiStop => {
                        self.twi_log.push_str("[P]");
                        self.twi_expect_addr = false;
                    }
                    IoKind::Data => {
                        if self.twi_expect_addr {
                            // First byte after START is the address + R/W bit.
                            let rw = if ev.byte & 1 == 1 { "R" } else { "W" };
                            self.twi_log.push_str(&format!("addr 0x{:02X}{} ", ev.byte >> 1, rw));
                            self.twi_expect_addr = false;
                        } else if ev.write {
                            self.twi_log.push_str(&format!("{:02X} ", ev.byte));
                        } else {
                            // A byte the device returned to the master.
                            self.twi_log.push_str(&format!("←{:02X} ", ev.byte));
                        }
                    }
                }
                cap_log(&mut self.twi_log);
            }
        }
    }

    /// Whether the active tab is an ik8b source. Only `.ik` files get compiled,
    /// linted and syntax-styled; any other file opens as plain text.
    pub fn active_is_ik(&self) -> bool {
        self.active_tab
            .and_then(|i| self.open_tabs.get(i))
            .map(|t| is_ik_file(&t.path))
            .unwrap_or(false)
    }

    /// All diagnostics to display: the compiler's (debounced) plus the IDE's
    /// instant lints on the active buffer. When both flag the same line, the
    /// precise lint wins so the squiggle lands on the exact token.
    pub fn all_diagnostics(&self) -> Vec<Diagnostic> {
        // Non-ik files are plain text: no language analysis at all.
        if !self.active_is_ik() {
            return Vec::new();
        }
        let lints = match self.active_tab.and_then(|i| self.open_tabs.get(i)) {
            Some(tab) => analysis::lint_buffer(&tab.content),
            None => Vec::new(),
        };
        let lint_lines: std::collections::HashSet<usize> = lints.iter().map(|d| d.line).collect();

        let mut out: Vec<Diagnostic> = self
            .diagnostics
            .iter()
            .filter(|d| d.line == 0 || !lint_lines.contains(&d.line))
            .cloned()
            .collect();
        out.extend(lints);
        out.sort_by_key(|d| d.line);
        out
    }

    /// Debounced live type-check: when the active buffer has been idle for
    /// `CHECK_DEBOUNCE`, run the compiler's front end in-process and refresh the
    /// diagnostics. Returns how long until the next check might fire, so the
    /// caller can schedule a repaint (egui is event-driven).
    pub fn maybe_run_check(&mut self) -> Option<Duration> {
        let idx = self.active_tab?;
        let tab = self.open_tabs.get(idx)?;
        // Plain-text files are never type-checked; clear any stale diagnostics.
        if !is_ik_file(&tab.path) {
            self.diagnostics.clear();
            return None;
        }
        let content = tab.content.clone();

        if content == self.last_checked_content {
            return None;
        }
        match self.last_edit {
            Some(t) if t.elapsed() >= CHECK_DEBOUNCE => {
                // Materialize any std modules this buffer imports so the
                // in-process front end can resolve them, then type-check.
                analysis::sync_std_imports(&content);
                // Fast and synchronous: it's the real lexer + parser, in-process.
                self.diagnostics = analysis::check(&content);
                // Diagnostics whose code lives in an imported module don't resolve
                // against this buffer (line 0). Locate them across the workspace so
                // the problem still carries a file + line to jump to.
                if let Some(ws) = self.workspace_dir.clone() {
                    for d in &mut self.diagnostics {
                        if d.line == 0 {
                            if let Some(raw) = d.raw.clone() {
                                if let Some((file, line)) = analysis::locate_in_workspace(&ws, &raw) {
                                    d.file = Some(file);
                                    d.line = line;
                                }
                            }
                        }
                    }
                }
                self.last_checked_content = content;
                None
            }
            Some(t) => Some(CHECK_DEBOUNCE.saturating_sub(t.elapsed())),
            None => Some(CHECK_DEBOUNCE),
        }
    }

    /// Jump to a diagnostic's location: open/focus its file (when it lives in a
    /// different one) and queue the editor to scroll to the line.
    pub fn goto_diagnostic(&mut self, file: &Option<PathBuf>, line: usize) {
        if let Some(f) = file {
            let already = self
                .active_tab
                .and_then(|i| self.open_tabs.get(i))
                .map(|t| &t.path)
                == Some(f);
            if !already {
                self.load_file(f.clone());
            }
        }
        if line > 0 {
            self.pending_scroll_line = Some(line);
        }
    }

    pub fn refresh_files(&mut self) {
        if let Some(dir) = &self.workspace_dir {
            let mut tree = workspace::scan_workspace(dir);
            tree.name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.workspace_tree = Some(tree);
        }
    }

    pub fn load_file(&mut self, path: PathBuf) {
        if let Some(idx) = self.open_tabs.iter().position(|t| t.path == path) {
            self.active_tab = Some(idx);
            return;
        }
        
        if let Ok(content) = std::fs::read_to_string(&path) {
            let last_mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
            self.open_tabs.push(OpenTab {
                path,
                content,
                is_modified: false,
                last_mtime,
                is_disk_different: false,
            });
            self.active_tab = Some(self.open_tabs.len() - 1);
        } else {
            self.terminal_output.push_str(&format!("{} Failed to load file: {:?}\n", now_ts(), path));
            self.show_terminal = true;
        }
    }

    pub fn save_active_file(&mut self) {
        if let Some(idx) = self.active_tab {
            let tab = &mut self.open_tabs[idx];
            let is_ik = is_ik_file(&tab.path);
            // Auto-format only ik8b sources — other files are saved verbatim so
            // the ik formatter never mangles them.
            if is_ik {
                tab.content = crate::syntax::format_code(&tab.content);
            }
            if let Err(e) = std::fs::write(&tab.path, &tab.content) {
                self.terminal_output.push_str(&format!("{} Failed to save file: {}\n", now_ts(), e));
                self.show_terminal = true;
            } else {
                tab.is_modified = false;
                tab.is_disk_different = false;
                tab.last_mtime = std::fs::metadata(&tab.path).ok().and_then(|m| m.modified().ok());
                self.terminal_output.push_str(&format!("{} Saved {:?}\n", now_ts(), tab.path));
                // Resource stats only make sense for ik8b sources.
                if is_ik {
                    runner::spawn_stats(tab.content.clone(), self.task_tx.clone());
                }
            }
        }
    }

    /// Start a cancellable task: mark the IDE busy, hand out a fresh cancel
    /// token, and swap in a fresh task channel so any straggler messages from a
    /// previously cancelled (but still-winding-down) worker are dropped instead
    /// of leaking into this run. `to_vm` selects which panel a later cancel
    /// notice is written to. Returns the token to pass to the worker.
    pub fn begin_task(&mut self, to_vm: bool) -> Arc<AtomicBool> {
        let (tx, rx) = mpsc::channel();
        self.task_tx = tx;
        self.task_rx = rx;
        self.is_busy = true;
        self.busy_writes_vm = to_vm;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        cancel
    }

    /// Cancel the in-flight task: signal its loops to stop, abandon its channel
    /// (so its final messages can't flip `is_busy` on the next run), and free the
    /// UI immediately. The detached worker exits on its own once it sees the flag.
    pub fn abandon_task(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.task_tx = tx;
        self.task_rx = rx;
        self.is_busy = false;
        let note = format!("{} ⏹ Cancelled.\n", now_ts());
        if self.busy_writes_vm {
            self.vm_output.push_str(&note);
        } else {
            self.terminal_output.push_str(&note);
        }
    }

    pub fn handle_background_tasks(&mut self) {
        while let Ok(msg) = self.task_rx.try_recv() {
            match msg {
                TaskMsg::Done => {
                    self.is_busy = false;
                    self.refresh_files();
                }
                TaskMsg::Compile(out) => {
                    self.terminal_output.push_str(&stamp(&out));
                }
                TaskMsg::Vm(out) => {
                    self.vm_output.push_str(&stamp(&out));
                }
                TaskMsg::VmResult(res) => {
                    self.vm_result = Some(res);
                }
                TaskMsg::Upload(out) => {
                    self.terminal_output.push_str(&stamp(&out));
                    self.show_terminal = true;
                }
                TaskMsg::Test(out) => {
                    self.terminal_output.push_str(&stamp(&out));
                    self.show_terminal = true;
                }
                TaskMsg::Stats(res) => {
                    self.stats_data = match res {
                        Ok(data) => Ok(Some(data)),
                        Err(e) => Err(e),
                    };
                }
            }
        }
    }

    pub fn trigger_burn_bootloader(&mut self) {
        let ubrr = (self.burn_f_cpu / (16 * self.burn_baud)).saturating_sub(1);
        let src = format!(
            "target {}\nimport std/bootloader\n@main {{ @bootloader_run({}) }}\n",
            self.burn_target, ubrr
        );

        crate::core::analysis::sync_std_imports(&src);
        match crate::core::analysis::compile(&src) {
            Ok(artifact) => {
                let _ = self.begin_task(false);
                self.show_terminal = true;
                self.terminal_output.clear();
                self.terminal_output.push_str(&format!("{} --- Burning Bootloader ---\n", now_ts()));
                
                crate::core::runner::spawn_burn_bootloader(
                    self.burn_path.clone(),
                    self.burn_target.clone(),
                    self.burn_programmer.clone(),
                    self.burn_port.clone(),
                    self.burn_baudrate.clone(),
                    self.burn_additional_flags.clone(),
                    artifact.hex,
                    self.task_tx.clone(),
                );
            }
            Err(diag) => {
                self.show_terminal = true;
                self.terminal_output.clear();
                self.terminal_output.push_str(&format!(
                    "{} Compilation failed — {}\n",
                    now_ts(),
                    diag.message
                ));
            }
        }
    }
    
    pub fn render_popups(&mut self, ctx: &egui::Context) {
        let mut close_popup = false;
        
        match &mut self.action_popup {
            ActionPopup::None => {}
            ActionPopup::Rename { path, new_name } => {
                egui::Window::new("Rename").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("✏ Rename").strong());
                    ui.separator();
                    let path_clone = path.clone();
                    let path_clone2 = path.clone();
                    ui.horizontal(|ui| {
                        ui.label("New name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let mut new_path = path_clone.clone();
                            new_path.set_file_name(&*new_name);
                            if std::fs::rename(&path_clone, new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let mut new_path = path_clone2.clone();
                            new_path.set_file_name(&*new_name);
                            if std::fs::rename(&path_clone2, new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::CreateFile { parent_dir, new_name } => {
                egui::Window::new("Create File").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("📄 Create File").strong());
                    ui.separator();
                    let dir_clone = parent_dir.clone();
                    let dir_clone2 = parent_dir.clone();
                    ui.horizontal(|ui| {
                        ui.label("File name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let new_path = dir_clone.join(&*new_name);
                            if std::fs::write(&new_path, "").is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let new_path = dir_clone2.join(&*new_name);
                            if std::fs::write(&new_path, "").is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::CreateDir { parent_dir, new_name } => {
                egui::Window::new("Create Directory").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("📁 Create Directory").strong());
                    ui.separator();
                    let dir_clone = parent_dir.clone();
                    let dir_clone2 = parent_dir.clone();
                    ui.horizontal(|ui| {
                        ui.label("Directory name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let new_path = dir_clone.join(&*new_name);
                            if std::fs::create_dir(&new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let new_path = dir_clone2.join(&*new_name);
                            if std::fs::create_dir(&new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::Delete { path } => {
                egui::Window::new("Delete").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("🗑 Confirm Delete").strong());
                    ui.separator();
                    ui.label(format!("Are you sure you want to delete {:?}?", path.file_name().unwrap_or_default()));
                    let path_clone = path.clone();
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            if path_clone.is_dir() {
                                let _ = std::fs::remove_dir_all(&path_clone);
                            } else {
                                let _ = std::fs::remove_file(&path_clone);
                            }
                            close_popup = true;
                        }
                        if ui.button("No").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
        }
        
        if close_popup {
            self.action_popup = ActionPopup::None;
            self.refresh_files();
        }
    }
    pub fn scan_examples(&mut self) {
        self.examples.clear();
        for (idx, ex) in EMBEDDED_EXAMPLES.iter().enumerate() {
            // Every embedded example is listed: a missing or malformed
            // info.json falls back to the directory name so a new example
            // never silently disappears from the menu.
            let mut info = ex
                .files
                .iter()
                .find(|f| f.name == "info.json")
                .and_then(|f| match serde_json::from_str::<ExampleInfo>(f.content) {
                    Ok(info) => Some(info),
                    Err(e) => {
                        eprintln!("ikide: bad info.json in example '{}': {}", ex.name, e);
                        None
                    }
                })
                .unwrap_or_else(|| ExampleInfo {
                    title: ex.name.to_string(),
                    description: String::new(),
                    embedded_index: 0,
                });
            info.embedded_index = idx;
            self.examples.push(info);
        }
        self.examples.sort_by(|a, b| a.title.cmp(&b.title));
    }
}

impl eframe::App for IkIdeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Periodically check if files have been modified on disk
        let now = Instant::now();
        if self.last_disk_check.elapsed() >= Duration::from_secs(1) {
            self.last_disk_check = now;
            let mut tabs_to_reload = Vec::new();
            for (idx, tab) in self.open_tabs.iter_mut().enumerate() {
                if let Ok(metadata) = std::fs::metadata(&tab.path) {
                    if let Ok(mtime) = metadata.modified() {
                        if let Some(last_time) = tab.last_mtime {
                            if mtime > last_time {
                                if !tab.is_modified {
                                    tabs_to_reload.push((idx, mtime));
                                } else {
                                    tab.is_disk_different = true;
                                }
                            }
                        } else {
                            tab.last_mtime = Some(mtime);
                        }
                    }
                }
            }
            for (idx, mtime) in tabs_to_reload {
                let tab = &mut self.open_tabs[idx];
                if let Ok(new_content) = std::fs::read_to_string(&tab.path) {
                    tab.content = new_content;
                    tab.last_mtime = Some(mtime);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_secs(1));

        // Keyboard shortcuts
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_active_file();
        }

        self.handle_background_tasks();

        // Stream any bytes the board sent into the serial monitor.
        self.pump_serial();
        if self.serial.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        // Drain live breadboard snapshots and keep animating while it runs.
        if self.pump_live() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Live, debounced background type-check of the active buffer.
        if let Some(wait) = self.maybe_run_check() {
            ctx.request_repaint_after(wait);
        }

        // Check shortcuts
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::R)) {
            if !self.is_busy && self.active_is_ik() {
                self.save_active_file();
                let _ = self.begin_task(false);
                self.show_terminal = true;
                self.terminal_output.clear();
                self.terminal_output.push_str(&format!("{} --- Compiling ---\n", now_ts()));
                let (path, content) = self.active_tab
                    .map(|idx| (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone()))
                    .unwrap_or((None, String::new()));
                runner::spawn_compile(self.workspace_dir.clone(), path, content, self.task_tx.clone());
            }
        }

        if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::S)) {
            if !self.is_busy && self.active_is_ik() {
                let cancel = self.begin_task(true);
                self.show_vm_trace = true;
                self.vm_output.clear();
                self.vm_result = None;
                self.vm_output.push_str(&format!("{} --- Simulation Starting ---\n", now_ts()));
                let (path, text) = if let Some(idx) = self.active_tab {
                    (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone())
                } else {
                    (None, String::new())
                };
                runner::spawn_simulate(self.workspace_dir.clone(), path, text, self.task_tx.clone(), self.sim_config(), cancel);
            }
        }

        // Handle incoming messagesult
        if let Some(rx) = &self.dialog_rx {
            if let Ok(folder_opt) = rx.try_recv() {
                self.dialog_rx = None;
                if let Some(folder) = folder_opt {
                    // Make the project root the process CWD so the compiler
                    // resolves local `import <module>` (resolved relative to the
                    // current directory) against the user's project.
                    let _ = std::env::set_current_dir(&folder);
                    self.workspace_dir = Some(folder);
                    self.open_tabs.clear();
                    self.active_tab = None;
                    self.refresh_files();
                    // Remember this project for the next launch.
                    self.persist();
                }
            }
        }
        
        self.render_popups(ctx);

        if self.is_busy || self.dialog_rx.is_some() {
            ctx.request_repaint();
        }

        if self.workspace_dir.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("Welcome to IKIDE");
                    ui.add_space(20.0);
                    if ui.add_enabled(self.dialog_rx.is_none(), egui::Button::new("📂 Open Project Folder")).clicked() {
                        self.open_folder_dialog();
                    }
                    if self.dialog_rx.is_some() {
                        ui.add_space(10.0);
                        ui.spinner();
                        ui.label("Waiting for folder selection...");
                    }
                });
            });
            return;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.add_enabled(self.dialog_rx.is_none(), egui::Button::new("📂 Open Project")).clicked() {
                        self.open_folder_dialog();
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("💾 Save (Ctrl+S)")).clicked() {
                        self.save_active_file();
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(!self.is_busy && self.active_is_ik(), egui::Button::new("✨ Format Code")).clicked() {
                        if let Some(idx) = self.active_tab {
                            let unformatted = self.open_tabs[idx].content.clone();
                            self.open_tabs[idx].content = crate::syntax::format_code(&unformatted);
                            self.open_tabs[idx].is_modified = true;
                        }
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_terminal, "Output");
                    ui.checkbox(&mut self.show_vm_trace, "Simulation");
                    ui.checkbox(&mut self.show_stats, "Resource Stats");
                    ui.checkbox(&mut self.show_minimap, "Minimap");
                    ui.checkbox(&mut self.show_serial, "Serial Monitor");
                    ui.checkbox(&mut self.show_breadboard, "Breadboard");
                    ui.separator();
                    if ui.add_enabled(!self.is_busy, egui::Button::new("🔄 Refresh Explorer")).clicked() {
                        self.refresh_files();
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Run", |ui| {
                    if ui.add_enabled(!self.is_busy && self.active_is_ik(), egui::Button::new("🔨 Compile (Shift+R)")).clicked() {
                        self.save_active_file();
                        let _ = self.begin_task(false);
                        self.show_terminal = true;
                        self.terminal_output.clear();
                        self.terminal_output.push_str(&format!("{} --- Compiling ---\n", now_ts()));
                        let (path, content) = self.active_tab
                            .map(|idx| (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone()))
                            .unwrap_or((None, String::new()));
                        runner::spawn_compile(self.workspace_dir.clone(), path, content, self.task_tx.clone());
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_is_ik(), egui::Button::new("🚀 Simulate (Shift+S)")).clicked() {
                        let cancel = self.begin_task(true);
                        self.show_vm_trace = true;
                        self.vm_output.clear();
                        self.vm_result = None;
                        self.vm_output.push_str(&format!("{} --- Simulation Starting ---\n", now_ts()));
                        let (path, text) = if let Some(idx) = self.active_tab {
                            (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone())
                        } else { (None, String::new()) };
                        runner::spawn_simulate(self.workspace_dir.clone(), path, text, self.task_tx.clone(), self.sim_config(), cancel);
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy, egui::Button::new("🧪 Run Tests")).clicked() {
                        self.save_active_file();
                        let cancel = self.begin_task(false);
                        self.show_terminal = true;
                        self.terminal_output.clear();
                        self.terminal_output.push_str(&format!("{} --- Running Tests ---\n", now_ts()));
                        let target = if self.avrdude_target.is_empty() {
                            "atmega328p".to_string()
                        } else {
                            self.avrdude_target.clone()
                        };
                        crate::core::testbed::spawn_run_tests(self.workspace_dir.clone(), target, self.task_tx.clone(), cancel);
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_is_ik(), egui::Button::new("🔌 Upload to Board")).clicked() {
                        self.save_active_file();
                        let _ = self.begin_task(false);
                        self.show_terminal = true;
                        self.terminal_output.clear();
                        let path = self.active_tab.map(|idx| self.open_tabs[idx].path.clone());
                        if self.use_bootloader {
                            self.terminal_output.push_str(&format!("{} --- Uploading (bootloader) ---\n", now_ts()));
                            runner::spawn_bootloader_upload(self.workspace_dir.clone(), path, self.bootloader_port.clone(), self.bootloader_baud, self.task_tx.clone());
                        } else {
                            self.terminal_output.push_str(&format!("{} --- Uploading (avrdude) ---\n", now_ts()));
                            runner::spawn_upload(self.workspace_dir.clone(), path, self.avrdude_path.clone(), self.avrdude_target.clone(), self.avrdude_programmer.clone(), self.avrdude_port.clone(), self.avrdude_baudrate.clone(), self.avrdude_additional_flags.clone(), self.task_tx.clone());
                        }
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Config", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.show_preferences = true;
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Help", |ui| {
                    if ui.button("📚 Built-in Examples").clicked() {
                        self.scan_examples();
                        self.show_examples = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("ℹ About IK IDE").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
                
                if self.is_busy {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⏹ Stop").on_hover_text("Cancel the running task").clicked() {
                            self.abandon_task();
                        }
                        ui.label("Working...");
                        ui.spinner();
                    });
                }
            });
        });

        if self.show_preferences {
            let mut show = self.show_preferences;
            let mut close = false;
            egui::Window::new("Preferences")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .open(&mut show)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("⚙ Preferences").strong());
                    ui.label(egui::RichText::new("Compiler & simulator are built in — only their settings are exposed.").weak().small());
                    ui.separator();

                    egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
                        egui::CollapsingHeader::new("Simulation").default_open(true).show(ui, |ui| {
                            ui.label(egui::RichText::new("Runs in-process — no external VM binary.").weak().small());
                            egui::Grid::new("sim_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                                ui.label("Max Instructions:");
                                ui.add(egui::DragValue::new(&mut self.vm_max_cycles).speed(1000).range(0..=1_000_000_000))
                                    .on_hover_text("Stop after this many executed instructions. 0 = run until the core halts.");
                                ui.end_row();

                                ui.label("Instruction Trace:");
                                ui.checkbox(&mut self.sim_trace, "Log each executed instruction (-t)")
                                    .on_hover_text("Capture a per-instruction trace into the log. Capped to keep the UI responsive.");
                                ui.end_row();

                                ui.label("Dump Registers:");
                                ui.checkbox(&mut self.sim_dump_regs, "Append register/flag dump to the log");
                                ui.end_row();

                                ui.label("Memory Peek Addr:");
                                ui.text_edit_singleline(&mut self.sim_peek_addr)
                                    .on_hover_text("Data-space address to read at the end of the run (0x-hex or decimal). Blank = off.");
                                ui.end_row();

                                ui.label("Peek Length:");
                                ui.add(egui::DragValue::new(&mut self.sim_peek_len).speed(1).range(1..=256));
                                ui.end_row();
                            });
                        });

                        ui.add_space(5.0);

                        ui.label(egui::RichText::new("Upload method").strong());
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.use_bootloader, true, "Bootloader")
                                .on_hover_text("Flash through the ik serial bootloader running on the board.");
                            ui.radio_value(&mut self.use_bootloader, false, "avrdude")
                                .on_hover_text("Flash with an external programmer via avrdude.");
                        });
                        ui.label(egui::RichText::new("\"Upload to Board\" uses the selected method.").small().weak());
                        ui.add_space(5.0);

                        egui::CollapsingHeader::new("Avrdude Configuration").default_open(!self.use_bootloader).show(ui, |ui| {
                            ui.add_enabled_ui(!self.use_bootloader, |ui| {
                            egui::Grid::new("avrdude_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                                ui.label("Executable Path:");
                                ui.text_edit_singleline(&mut self.avrdude_path);
                                ui.end_row();

                                ui.label("Target MCU:");
                                let items: Vec<(String, String)> = self.devices.iter().map(|(d, _)| (d.clone(), d.clone())).collect();
                                let sel = if self.avrdude_target.is_empty() { "Select MCU...".to_string() } else { self.avrdude_target.clone() };
                                if let Some(d) = crate::ui::widgets::filter_combo(ui, "mcu_target", &sel, &items) {
                                    self.avrdude_target = d;
                                }
                                ui.end_row();
                                
                                ui.label("Programmer:");
                                egui::ComboBox::from_id_salt("programmer_target")
                                    .selected_text(&self.avrdude_programmer)
                                    .show_ui(ui, |ui| {
                                        for p in ["arduino", "usbasp", "usbtiny", "avrispmkII", "stk500v1", "stk500v2", "micronucleus"] {
                                            ui.selectable_value(&mut self.avrdude_programmer, p.to_string(), p);
                                        }
                                    });
                                ui.end_row();
                                
                                ui.label("Port:");
                                ui.text_edit_singleline(&mut self.avrdude_port);
                                ui.end_row();

                                ui.label("Baudrate (opt):");
                                ui.text_edit_singleline(&mut self.avrdude_baudrate);
                                ui.end_row();
                                
                                ui.label("Additional Flags:");
                                ui.text_edit_singleline(&mut self.avrdude_additional_flags);
                                ui.end_row();
                            });
                            });
                        });

                        ui.add_space(5.0);
                        egui::CollapsingHeader::new("Bootloader Upload").default_open(true).show(ui, |ui| {
                            ui.label(egui::RichText::new(
                                "Flash through the ik serial bootloader running on the board.",
                            ).small().weak());
                            egui::Grid::new("bootloader_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                                ui.label("Serial Port:");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.bootloader_port)
                                            .desired_width(160.0)
                                            .hint_text("/dev/ttyUSB0"),
                                    );
                                    egui::ComboBox::from_id_salt("bootloader_port_pick")
                                        .selected_text("▾")
                                        .width(16.0)
                                        .show_ui(ui, |ui| {
                                            let ports = crate::core::serial::list_ports();
                                            if ports.is_empty() {
                                                ui.label(egui::RichText::new("no ports found").weak());
                                            }
                                            for p in ports {
                                                ui.selectable_value(&mut self.bootloader_port, p.clone(), p);
                                            }
                                        });
                                });
                                ui.end_row();

                                ui.label("Baud:");
                                let mut baud_str = self.bootloader_baud.to_string();
                                if ui.text_edit_singleline(&mut baud_str).changed() {
                                    if let Ok(b) = baud_str.trim().parse::<u32>() {
                                        self.bootloader_baud = b;
                                    }
                                }
                                ui.end_row();
                            });
                            ui.label(egui::RichText::new(
                                "Baud must match the bootloader's BL_UBRR (default 9600 @ 8 MHz).",
                            ).small().weak());
                        });

                        ui.add_space(5.0);
                        egui::CollapsingHeader::new("Burn Bootloader").default_open(false).show(ui, |ui| {
                            ui.label(egui::RichText::new(
                                "Compile the standard bootloader for a target chip and write it using avrdude with fuses.",
                            ).small().weak());
                            
                            egui::Grid::new("burn_bootloader_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                                ui.label("Executable Path:");
                                ui.text_edit_singleline(&mut self.burn_path);
                                ui.end_row();

                                ui.label("Target MCU:");
                                let items: Vec<(String, String)> = self
                                    .devices
                                    .iter()
                                    .filter(|(d, _)| crate::core::bootloader::has_bootloader_support(d))
                                    .map(|(d, _)| (d.clone(), d.clone()))
                                    .collect();
                                let display_text = if self.burn_target.is_empty() { "Select MCU...".to_string() } else { self.burn_target.clone() };
                                if let Some(d) = crate::ui::widgets::filter_combo(ui, "burn_target_pick", &display_text, &items) {
                                    self.burn_target = d;
                                    self.burn_additional_flags = crate::core::bootloader::suggest_burn_fuse_flags(&self.burn_target);
                                }
                                ui.end_row();

                                ui.label("Clock F_CPU (Hz):");
                                ui.add(egui::DragValue::new(&mut self.burn_f_cpu).speed(1000000).range(1_000_000..=32_000_000))
                                    .on_hover_text("Calculates the bootloader UART divisor based on this crystal/internal clock frequency.");
                                ui.end_row();

                                ui.label("Baudrate (bps):");
                                let mut baud_str = self.burn_baud.to_string();
                                if ui.text_edit_singleline(&mut baud_str).changed() {
                                    if let Ok(b) = baud_str.trim().parse::<u32>() {
                                        self.burn_baud = b;
                                    }
                                }
                                ui.end_row();

                                ui.label("Programmer:");
                                egui::ComboBox::from_id_salt("burn_programmer_pick")
                                    .selected_text(&self.burn_programmer)
                                    .show_ui(ui, |ui| {
                                        for p in ["arduino", "usbasp", "usbtiny", "avrispmkII", "stk500v1", "stk500v2", "micronucleus"] {
                                            ui.selectable_value(&mut self.burn_programmer, p.to_string(), p);
                                        }
                                    });
                                ui.end_row();

                                ui.label("Port:");
                                ui.text_edit_singleline(&mut self.burn_port);
                                ui.end_row();

                                ui.label("Baudrate (opt):");
                                ui.text_edit_singleline(&mut self.burn_baudrate);
                                ui.end_row();

                                ui.label("Additional Flags:");
                                ui.text_edit_singleline(&mut self.burn_additional_flags)
                                    .on_hover_text("Specify avrdude flags here, including fuses (e.g. -U lfuse:w:0xFF:m -U hfuse:w:0xD8:m).");
                                ui.end_row();
                            });

                            ui.add_space(5.0);
                            ui.horizontal(|ui| {
                                let can_burn = !self.is_busy && !self.burn_target.is_empty();
                                ui.add_enabled_ui(can_burn, |ui| {
                                    if ui.button("🔥 Burn Bootloader").clicked() {
                                        self.trigger_burn_bootloader();
                                    }
                                });
                            });
                        });
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save & Close").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                // Write the updated preferences to disk.
                self.persist();
                show = false;
            }
            self.show_preferences = show;
        }

        if self.show_about {
            let mut show = self.show_about;
            egui::Window::new("About")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .default_width(450.0)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("ℹ About IK IDE").strong().size(18.0));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Version 0.1.0").weak());
                    ui.separator();
                    
                    ui.label(egui::RichText::new("Vision").strong());
                    ui.label("IK was created to push the true capabilities of 8-bit AVR microcontrollers to their limits. By breaking free from traditional, heavy abstractions, it empowers developers to deeply understand and optimize their hardware at the lowest level.");
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("Purpose").strong());
                    ui.label("A core goal of the IK language is true portability. It is designed to target a common subset of AVR instructions, ensuring that code can run seamlessly across a wide variety of chips without requiring massive rewrites when changing hardware.");
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("Philosophy").strong());
                    ui.label("True understanding comes from looking 'under the hood'. Built entirely in Rust for frictionless cross-platform development, the IK ecosystem provides cycle-accurate simulation, real-time resource statistics, and transparent bootloader flashing in a single cohesive environment.");
                    ui.add_space(8.0);
                    
                    ui.label(egui::RichText::new("Project Links").strong());
                    ui.horizontal(|ui| {
                        ui.label("•");
                        ui.hyperlink_to("ikide", "https://github.com/isakruas/ikide");
                        ui.label("- The IDE workspace");
                    });
                    ui.horizontal(|ui| {
                        ui.label("•");
                        ui.hyperlink_to("ik8b", "https://github.com/isakruas/ik8b");
                        ui.label("- The core compiler");
                    });
                    ui.horizontal(|ui| {
                        ui.label("•");
                        ui.hyperlink_to("ik8bvm", "https://github.com/isakruas/ik8bvm");
                        ui.label("- The cycle-accurate simulator");
                    });
                    ui.add_space(8.0);
                    
                    ui.label("Authors: The IK Authors");
                    ui.label("License: Apache-2.0");
                    ui.separator();
                    if ui.button("Close").clicked() {
                        show = false;
                    }
                });
            self.show_about = show;
        }

        if self.show_examples {
            let mut show = self.show_examples;
            let mut load_example_idx = None;
            egui::Window::new("Examples Library")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .default_width(600.0)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("📚 Examples Library").strong());
                    ui.label(egui::RichText::new("Click an example to load it into your current workspace.").weak().small());
                    ui.separator();
                    
                    if self.examples.is_empty() {
                        ui.label("No examples found.");
                    } else {
                        // Search Input
                        ui.horizontal(|ui| {
                            ui.label("🔍 Search:");
                            let old_search = self.example_search.clone();
                            ui.text_edit_singleline(&mut self.example_search);
                            if self.example_search != old_search {
                                self.example_page = 0;
                            }
                            if !self.example_search.is_empty() {
                                if ui.button("Clear").clicked() {
                                    self.example_search.clear();
                                    self.example_page = 0;
                                }
                            }
                        });
                        ui.add_space(6.0);
                        
                        let query = self.example_search.trim().to_lowercase();
                        let filtered: Vec<&ExampleInfo> = self.examples.iter()
                            .filter(|ex| {
                                query.is_empty() || ex.title.to_lowercase().contains(&query) || ex.description.to_lowercase().contains(&query)
                            })
                            .collect();
                        
                        let page_size = 4;
                        let total_items = filtered.len();
                        let total_pages = (total_items + page_size - 1).max(1) / page_size;
                        
                        if self.example_page >= total_pages {
                            self.example_page = 0;
                        }
                        
                        let start_idx = self.example_page * page_size;
                        let end_idx = std::cmp::min(start_idx + page_size, total_items);
                        
                        if filtered.is_empty() {
                            ui.label("No examples found matching your search.");
                        } else {
                            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                for ex in &filtered[start_idx..end_idx] {
                                    let original_idx = ex.embedded_index;
                                    ui.group(|ui| {
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(format!("🚀 {}", ex.title)).strong());
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.add_enabled_ui(self.workspace_dir.is_some(), |ui| {
                                                        if ui.button("Load Example").clicked() {
                                                            load_example_idx = Some(original_idx);
                                                        }
                                                    });
                                                });
                                            });
                                            ui.label(&ex.description);
                                        });
                                    });
                                    ui.add_space(6.0);
                                }
                            });
                            
                            if total_pages > 1 {
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(self.example_page > 0, |ui| {
                                        if ui.button("◀ Previous").clicked() {
                                            self.example_page -= 1;
                                        }
                                    });
                                    
                                    ui.label(format!("Page {} of {}", self.example_page + 1, total_pages));
                                    ui.label(format!("(Showing {}-{} of {})", start_idx + 1, end_idx, total_items));
                                    
                                    ui.add_enabled_ui(self.example_page + 1 < total_pages, |ui| {
                                        if ui.button("Next ▶").clicked() {
                                            self.example_page += 1;
                                        }
                                    });
                                });
                            }
                        }
                    }
                    
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            show = false;
                        }
                        if self.workspace_dir.is_none() {
                            ui.label(egui::RichText::new("⚠️ Open a folder workspace first to load examples.").weak().small());
                        }
                    });
                });
            
            if let Some(idx) = load_example_idx {
                if let Some(dir) = &self.workspace_dir {
                    let ex_embedded = &EMBEDDED_EXAMPLES[idx];
                    for file in ex_embedded.files {
                        if file.name == "info.json" {
                            continue;
                        }
                        let target_path = dir.join(file.name);
                        if let Some(parent) = target_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::write(&target_path, file.content);
                    }
                    self.refresh_files();
                }
                show = false;
            }
            self.show_examples = show;
        }

        if self.show_terminal {
            terminal::render(self, ctx);
        }
        explorer::render(self, ctx);
        if self.show_vm_trace {
            right_panel::render(self, ctx);
        }
        crate::ui::serial::render(self, ctx);
        crate::ui::breadboard::render(self, ctx);
        editor::render(self, ctx);
    }

    /// Persist preferences, layout and the open project when the window closes.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }
}
