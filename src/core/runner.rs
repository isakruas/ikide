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
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use serde::Deserialize;

use ik8bvm::core::AvrVm;
use ik8bvm::devices::{AvrCoreClass, AVR_DEVICE_TABLE};

use crate::core::analysis::{self, BuildArtifact};

/// Generic fallback memory map, used when the target chip is not in the
/// device table (mirrors the ik8bvm CLI's defaults).
const GENERIC_FLASH_BYTES: u32 = 256 * 1024;
const GENERIC_SRAM_BYTES: u32 = 16 * 1024;
const GENERIC_EEPROM_BYTES: u32 = 4 * 1024;
const GENERIC_SRAM_START: u32 = 0x100;

/// Where the build writes its HEX: `<workspace>/build/<name>.hex`, or next to
/// the source when there is no workspace.
fn out_hex_path(workspace_dir: &Option<PathBuf>, path: &PathBuf) -> PathBuf {
    if let Some(ws) = workspace_dir {
        let build_dir = ws.join("build");
        let _ = std::fs::create_dir_all(&build_dir);
        build_dir.join(path.file_name().unwrap()).with_extension("hex")
    } else {
        path.with_extension("hex")
    }
}

fn stats_from(a: &BuildArtifact) -> StatsData {
    let pct = |used: u32, total: u32| if total == 0 { 0 } else { used * 100 / total };
    StatsData {
        target_name: a.device.clone(),
        target_core: a.core.clone(),
        prog_used: a.prog_used,
        prog_total: a.prog_total,
        prog_pct: pct(a.prog_used, a.prog_total),
        sram_used: a.sram_used,
        sram_total: a.sram_total,
        sram_pct: pct(a.sram_used, a.sram_total),
        eeprom_used: a.eeprom_used,
        eeprom_total: a.eeprom_total,
        eeprom_pct: pct(a.eeprom_used, a.eeprom_total),
        regs_used: a.regs_used,
        regs_total: a.regs_total,
        spills: a.spills,
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct StatsData {
    pub target_name: String,
    pub target_core: String,
    pub prog_used: u32,
    pub prog_total: u32,
    pub prog_pct: u32,
    pub sram_used: u32,
    pub sram_total: u32,
    pub sram_pct: u32,
    pub eeprom_used: u32,
    pub eeprom_total: u32,
    pub eeprom_pct: u32,
    #[serde(default)]
    pub regs_used: u32,
    #[serde(default)]
    pub regs_total: u32,
    #[serde(default)]
    pub spills: u32,
}

/// Structured snapshot of the simulated core at the end of a run, so the IDE
/// can show a readable state panel instead of only raw text.
#[derive(Clone, Debug)]
pub struct VmResult {
    pub device: String,
    pub core: String,
    pub executed: u64,
    pub cycles: u64,
    pub pc: u32,
    pub sp: u16,
    pub sreg: u8,
    pub regs: [u8; 32],
    pub flash_bytes: u32,
    pub sram_bytes: u32,
    pub eeprom_bytes: u32,
    pub halt_reason: String,
}

/// Hard cap on captured trace lines, so a long run can't flood the IDE panel.
const TRACE_LINE_LIMIT: usize = 50_000;

/// User-tunable knobs for an in-process simulation run (set in Preferences).
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// Stop after this many executed instructions (0 = run until the core halts).
    pub max_instr: u32,
    /// Capture a per-instruction trace into the log (the `-t` flag equivalent).
    pub trace: bool,
    /// Append a full register/flag dump to the simulation log.
    pub dump_regs: bool,
    /// Optional data-space address to read out at the end of the run.
    pub peek_addr: Option<u32>,
    /// How many consecutive bytes to read from `peek_addr`.
    pub peek_len: u32,
}

pub enum TaskMsg {
    Compile(String),
    Vm(String),
    VmResult(VmResult),
    Upload(String),
    Test(String),
    Stats(Result<StatsData, String>),
    Done,
}

/// Human-readable name for an AVR core class (matches the ik8bvm CLI labels).
fn core_name(core: AvrCoreClass) -> &'static str {
    match core {
        AvrCoreClass::RC => "AVRrc",
        AvrCoreClass::E => "AVRe",
        AvrCoreClass::EP => "AVRe+",
        AvrCoreClass::XT => "AVRxt",
        AvrCoreClass::XM => "AVRxm",
        AvrCoreClass::Unknown => "generic",
    }
}

/// Build a fresh VM for `device`, preloaded with that chip's memory/core
/// config from the compiler's device table, or a generic map as a fallback.
pub fn build_vm(device: &str) -> AvrVm {
    if let Some(dev) = AVR_DEVICE_TABLE.iter().find(|d| d.name == device) {
        let mut vm = AvrVm::new(
            device.to_string(),
            dev.core,
            dev.flash_bytes,
            dev.sram_bytes,
            dev.eeprom_bytes,
            dev.sram_start,
        );
        vm.sp = dev.ram_end as u16;
        vm
    } else {
        AvrVm::new(
            "generic".to_string(),
            AvrCoreClass::Unknown,
            GENERIC_FLASH_BYTES,
            GENERIC_SRAM_BYTES,
            GENERIC_EEPROM_BYTES,
            GENERIC_SRAM_START,
        )
    }
}

/// Render a register/flag/state dump as text (the same view the CLI printed
/// with `-d`, kept in-process so it lands in the IDE log).
fn format_dump(vm: &AvrVm) -> String {
    let mut s = String::new();
    s.push_str("Registers:\n");
    for i in 0..32 {
        s.push_str(&format!("R{:<2} = 0x{:02X}", i, vm.r[i]));
        if i % 4 == 3 {
            s.push('\n');
        } else {
            s.push_str(" | ");
        }
    }
    s.push_str(&format!("PC   = 0x{:06X}\n", vm.pc));
    s.push_str(&format!("SP   = 0x{:04X}\n", vm.sp));
    s.push_str(&format!(
        "SREG = 0x{:02X}  [{}{}{}{}{}{}{}{}]\n",
        vm.sreg,
        if vm.sreg & 0x80 != 0 { 'I' } else { '-' },
        if vm.sreg & 0x40 != 0 { 'T' } else { '-' },
        if vm.sreg & 0x20 != 0 { 'H' } else { '-' },
        if vm.sreg & 0x10 != 0 { 'S' } else { '-' },
        if vm.sreg & 0x08 != 0 { 'V' } else { '-' },
        if vm.sreg & 0x04 != 0 { 'N' } else { '-' },
        if vm.sreg & 0x02 != 0 { 'Z' } else { '-' },
        if vm.sreg & 0x01 != 0 { 'C' } else { '-' }
    ));
    s.push_str(&format!("Cycles = {}\n", vm.cycles));
    s
}

/// Compile the buffer in-process and write the HEX. Reports the build result
/// and the resource usage; no compiler binary is involved.
pub fn spawn_compile(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, content: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);
            analysis::sync_std_imports(&content);
            match analysis::compile(&content) {
                Ok(artifact) => match std::fs::write(&out_hex, &artifact.hex) {
                    Ok(_) => {
                        for w in &artifact.warnings {
                            let loc = if w.line > 0 { format!("line {}: ", w.line) } else { String::new() };
                            let _ = tx.send(TaskMsg::Compile(format!("Warning — {}{}\n", loc, w.message)));
                        }
                        let _ = tx.send(TaskMsg::Compile(format!("Compilation successful. Output at {:?}\n", out_hex)));
                        let _ = tx.send(TaskMsg::Stats(Ok(stats_from(&artifact))));
                    }
                    Err(e) => {
                        let _ = tx.send(TaskMsg::Compile(format!("Failed to write HEX: {}\n", e)));
                    }
                },
                Err(diag) => {
                    let loc = if diag.line > 0 { format!("line {}: ", diag.line) } else { String::new() };
                    let _ = tx.send(TaskMsg::Compile(format!("Compilation failed — {}{}\n", loc, diag.message)));
                    let _ = tx.send(TaskMsg::Stats(Err(diag.message)));
                }
            }
        } else {
            let _ = tx.send(TaskMsg::Compile("No file selected to compile.\n".to_string()));
        }
        let _ = tx.send(TaskMsg::Done);
    });
}

/// Recompute resource usage (used on save) in-process, without touching disk.
pub fn spawn_stats(content: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        analysis::sync_std_imports(&content);
        match analysis::compile(&content) {
            Ok(artifact) => {
                let _ = tx.send(TaskMsg::Stats(Ok(stats_from(&artifact))));
            }
            Err(diag) => {
                let _ = tx.send(TaskMsg::Stats(Err(diag.message)));
            }
        }
    });
}

/// Compile in-process, then simulate the HEX in-process with the ik8bvm
/// library — no external VM binary. Streams a readable log plus a structured
/// end-of-run snapshot (`VmResult`) for the state panel.
pub fn spawn_simulate(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, content: String, tx: Sender<TaskMsg>, cfg: SimConfig, cancel: Arc<AtomicBool>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);

            let _ = tx.send(TaskMsg::Vm("Compiling (in-process)...\n".to_string()));
            analysis::sync_std_imports(&content);
            let artifact = match analysis::compile(&content) {
                Ok(a) => a,
                Err(diag) => {
                    let loc = if diag.line > 0 { format!("line {}: ", diag.line) } else { String::new() };
                    let _ = tx.send(TaskMsg::Vm(format!("Compilation failed — {}{}\nAborting simulation.\n", loc, diag.message)));
                    let _ = tx.send(TaskMsg::Done);
                    return;
                }
            };
            for w in &artifact.warnings {
                let loc = if w.line > 0 { format!("line {}: ", w.line) } else { String::new() };
                let _ = tx.send(TaskMsg::Vm(format!("Warning — {}{}\n", loc, w.message)));
            }
            if let Err(e) = std::fs::write(&out_hex, &artifact.hex) {
                let _ = tx.send(TaskMsg::Vm(format!("Failed to write HEX: {}\n", e)));
                let _ = tx.send(TaskMsg::Done);
                return;
            }
            let _ = tx.send(TaskMsg::Stats(Ok(stats_from(&artifact))));

            let target = artifact.device.clone();

            // Build and preload the in-process core for this target.
            let mut vm = build_vm(&target);
            vm.trace = cfg.trace;
            vm.trace_limit = TRACE_LINE_LIMIT;
            let _ = tx.send(TaskMsg::Vm(format!(
                "Target: {} ({})  flash={}B SRAM={}B EEPROM={}B\n",
                vm.device, core_name(vm.core), vm.flash_bytes, vm.sram_bytes, vm.eeprom_bytes
            )));

            if let Err(e) = ik8bvm::hw::load_hex(&mut vm, &out_hex.to_string_lossy()) {
                let _ = tx.send(TaskMsg::Vm(format!("Failed to load HEX: {}\n", e)));
                let _ = tx.send(TaskMsg::Done);
                return;
            }

            let _ = tx.send(TaskMsg::Vm("--- Running simulation ---\n".to_string()));

            // Step until the core halts or hits the instruction budget. Poll the
            // cancel flag every so often so "Stop" interrupts a long run promptly
            // without paying an atomic load on every single instruction.
            let max = cfg.max_instr as u64;
            let mut executed: u64 = 0;
            let mut cancelled = false;
            while vm.running {
                vm.step();
                executed += 1;
                if max > 0 && executed >= max {
                    break;
                }
                if executed & 0xFFFF == 0 && cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
            }
            if cancelled {
                let _ = tx.send(TaskMsg::Vm(format!(
                    "\n Cancelled after {} instructions.\n",
                    executed
                )));
                let _ = tx.send(TaskMsg::Done);
                return;
            }

            // Stream the captured instruction trace (the `-t` equivalent).
            if cfg.trace {
                let mut block = String::from("--- Trace ---\n");
                for line in &vm.trace_buf {
                    block.push_str(line);
                    block.push('\n');
                }
                if vm.trace_truncated {
                    block.push_str(&format!("... trace truncated at {} lines\n", vm.trace_buf.len()));
                }
                block.push_str("-------------\n");
                let _ = tx.send(TaskMsg::Vm(block));
            }

            let halt_reason = if vm.unknown_opcode {
                format!("Halted on unknown opcode at PC=0x{:06X}", vm.pc)
            } else if !vm.running {
                "Program halted (sleep / RJMP .-2)".to_string()
            } else {
                format!("Reached instruction limit ({})", max)
            };
            let _ = tx.send(TaskMsg::Vm(format!(
                "{}\nExecuted {} instructions, {} cycles.\n",
                halt_reason, executed, vm.cycles
            )));

            if cfg.dump_regs {
                let _ = tx.send(TaskMsg::Vm(format_dump(&vm)));
            }

            if let Some(addr) = cfg.peek_addr {
                let mut s = String::from("Memory peek:\n");
                for k in 0..cfg.peek_len.max(1) {
                    let a = addr + k;
                    s.push_str(&format!("  MEM[0x{:04X}] = 0x{:02X}\n", a, vm.read_data(a)));
                }
                let _ = tx.send(TaskMsg::Vm(s));
            }

            // Hand the IDE a structured snapshot for the state panel.
            let _ = tx.send(TaskMsg::VmResult(VmResult {
                device: vm.device.clone(),
                core: core_name(vm.core).to_string(),
                executed,
                cycles: vm.cycles,
                pc: vm.pc,
                sp: vm.sp,
                sreg: vm.sreg,
                regs: vm.r,
                flash_bytes: vm.flash_bytes,
                sram_bytes: vm.sram_bytes,
                eeprom_bytes: vm.eeprom_bytes,
                halt_reason,
            }));

            let _ = tx.send(TaskMsg::Vm("\nSimulation complete.\n".to_string()));
        } else {
            let _ = tx.send(TaskMsg::Vm("No file selected to simulate.\n".to_string()));
        }
        let _ = tx.send(TaskMsg::Done);
    });
}

/// Translate a compiler/device-table name (e.g. `atmega32a`, `attiny85`) into
/// the part id avrdude expects on `-p` (e.g. `m32`, `t85`). avrdude does not
/// accept the long device names, so we map family prefixes to its short ids
/// and collapse the silicon-revision `a` variants that share a base part.
pub fn avrdude_part(name: &str) -> String {
    let n = name.trim().to_lowercase();

    // Revision variants avrdude folds into one base part id.
    const COLLAPSE: &[(&str, &str)] = &[
        ("atmega8a", "m8"),
        ("atmega16a", "m16"),
        ("atmega32a", "m32"),
        ("atmega64a", "m64"),
        ("atmega128a", "m128"),
        ("atmega8515", "m8515"),
        ("atmega8535", "m8535"),
    ];
    if let Some((_, id)) = COLLAPSE.iter().find(|(k, _)| *k == n) {
        return id.to_string();
    }

    // Family prefix → avrdude short prefix.
    const FAMILIES: &[(&str, &str)] = &[
        ("atxmega", "x"),
        ("atmega", "m"),
        ("attiny", "t"),
        ("at90usb", "usb"),
        ("at90can", "c"),
        ("at90pwm", "pwm"),
        ("at90s", ""),
    ];
    for (prefix, short) in FAMILIES {
        if let Some(rest) = n.strip_prefix(prefix) {
            return format!("{}{}", short, rest);
        }
    }

    // Unknown shape: pass through (also covers ids already in avrdude form).
    n
}

#[cfg(test)]
mod avrdude_tests {
    use super::avrdude_part;

    #[test]
    fn maps_families_and_revisions_to_avrdude_part_ids() {
        assert_eq!(avrdude_part("atmega32a"), "m32"); // revision collapses to base
        assert_eq!(avrdude_part("atmega328p"), "m328p");
        assert_eq!(avrdude_part("atmega2560"), "m2560");
        assert_eq!(avrdude_part("attiny85"), "t85");
        assert_eq!(avrdude_part("atxmega128a1"), "x128a1");
        assert_eq!(avrdude_part("at90usb1286"), "usb1286");
        assert_eq!(avrdude_part("at90can128"), "c128");
        assert_eq!(avrdude_part("at90s8515"), "8515");
        assert_eq!(avrdude_part("m328p"), "m328p"); // already an id: pass through
    }
}

/// Flash the already-built HEX through the ik serial bootloader running on the
/// board, instead of an external programmer.
pub fn spawn_bootloader_upload(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, port: String, baud: u32, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);
            let _ = tx.send(TaskMsg::Upload(format!(
                "Flashing {:?} via the serial bootloader on {} @ {} baud…\n",
                out_hex, port, baud
            )));
            let log_tx = tx.clone();
            let log = move |line: String| {
                let _ = log_tx.send(TaskMsg::Upload(format!("{}\n", line)));
            };
            match crate::core::bootloader::upload(&port, baud, &out_hex, &log) {
                Ok(()) => {}
                Err(e) => {
                    let _ = tx.send(TaskMsg::Upload(format!("Bootloader upload failed: {}\n", e)));
                }
            }
        } else {
            let _ = tx.send(TaskMsg::Upload("No file selected to upload.\n".to_string()));
        }
        let _ = tx.send(TaskMsg::Done);
    });
}

pub fn spawn_upload(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, avrdude_path: String, target: String, programmer: String, port: String, baudrate: String, additional_flags: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);
            let part = avrdude_part(&target);
            let _ = tx.send(TaskMsg::Upload(format!("Uploading {:?} to board (target {} -> avrdude -p{})...\n", out_hex, target, part)));

            // avrdude command using the converted part id and preferences
            let mut cmd = Command::new(avrdude_path);
            cmd.arg(format!("-p{}", part))
               .arg(format!("-c{}", programmer));
               
            if !port.is_empty() {
                cmd.arg(format!("-P{}", port));
            }
            if !baudrate.is_empty() {
                if baudrate.starts_with('-') {
                    for flag in baudrate.split_whitespace() {
                        cmd.arg(flag);
                    }
                } else {
                    cmd.arg(format!("-b{}", baudrate));
                }
            }
            
            for flag in additional_flags.split_whitespace() {
                cmd.arg(flag);
            }
            
            cmd.arg(format!("-Uflash:w:{}:i", out_hex.display()));

            match cmd.output() {
                Ok(output) => {
                    let _ = tx.send(TaskMsg::Upload(String::from_utf8_lossy(&output.stdout).into_owned()));
                    let _ = tx.send(TaskMsg::Upload(String::from_utf8_lossy(&output.stderr).into_owned()));
                    let _ = tx.send(TaskMsg::Upload("\nUpload complete.\n".to_string()));
                }
                Err(e) => {
                    let _ = tx.send(TaskMsg::Upload(format!("Failed to run avrdude: {}\n", e)));
                }
            }
        } else {
            let _ = tx.send(TaskMsg::Upload("No file selected to upload.\n".to_string()));
        }
        let _ = tx.send(TaskMsg::Done);
    });
}

pub fn spawn_burn_bootloader(
    avrdude_path: String,
    target: String,
    programmer: String,
    port: String,
    baudrate: String,
    additional_flags: String,
    hex_content: String,
    tx: Sender<TaskMsg>,
) {
    thread::spawn(move || {
        let part = avrdude_part(&target);
        let _ = tx.send(TaskMsg::Upload(format!(
            "Burning bootloader for target {} -> avrdude -p{}...\n",
            target, part
        )));

        // Create temporary HEX file.
        let temp_dir = std::env::temp_dir();
        let temp_hex_path = temp_dir.join("ik_bootloader.hex");
        if let Err(e) = std::fs::write(&temp_hex_path, &hex_content) {
            let _ = tx.send(TaskMsg::Upload(format!("Failed to write temporary HEX: {}\n", e)));
            let _ = tx.send(TaskMsg::Done);
            return;
        }

        // avrdude command using the converted part id and preferences
        let mut cmd = Command::new(avrdude_path);
        cmd.arg(format!("-p{}", part))
           .arg(format!("-c{}", programmer));
           
        if !port.is_empty() {
            cmd.arg(format!("-P{}", port));
        }
        if !baudrate.is_empty() {
            if baudrate.starts_with('-') {
                for flag in baudrate.split_whitespace() {
                    cmd.arg(flag);
                }
            } else {
                cmd.arg(format!("-b{}", baudrate));
            }
        }
        
        for flag in additional_flags.split_whitespace() {
            cmd.arg(flag);
        }

        // Hex file flash write
        cmd.arg(format!("-Uflash:w:{}:i", temp_hex_path.display()));

        match cmd.output() {
            Ok(output) => {
                let _ = tx.send(TaskMsg::Upload(String::from_utf8_lossy(&output.stdout).into_owned()));
                let _ = tx.send(TaskMsg::Upload(String::from_utf8_lossy(&output.stderr).into_owned()));
                let _ = tx.send(TaskMsg::Upload("\nBootloader burning complete.\n".to_string()));
            }
            Err(e) => {
                let _ = tx.send(TaskMsg::Upload(format!("Failed to run avrdude: {}\n", e)));
            }
        }

        // Clean up temporary HEX file
        let _ = std::fs::remove_file(temp_hex_path);
        let _ = tx.send(TaskMsg::Done);
    });
}
