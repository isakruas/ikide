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
use std::sync::mpsc::Sender;
use std::thread;
use serde::Deserialize;

use crate::core::analysis::{self, BuildArtifact};

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

pub enum TaskMsg {
    Compile(String),
    Vm(String),
    Upload(String),
    Stats(Result<StatsData, String>),
    Done,
}

/// Compile the buffer in-process and write the HEX. Reports the build result
/// and the resource usage; no compiler binary is involved.
pub fn spawn_compile(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, content: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);
            match analysis::compile(&content) {
                Ok(artifact) => match std::fs::write(&out_hex, &artifact.hex) {
                    Ok(_) => {
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

/// Compile in-process, then hand the HEX to the external avr-vm (written in C)
/// for simulation. The VM binary is the only external process that remains.
pub fn spawn_simulate(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, content: String, tx: Sender<TaskMsg>, vm_path: String, max_cycles: u32) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);

            let _ = tx.send(TaskMsg::Vm("Compiling (in-process)...\n".to_string()));
            let artifact = match analysis::compile(&content) {
                Ok(a) => a,
                Err(diag) => {
                    let loc = if diag.line > 0 { format!("line {}: ", diag.line) } else { String::new() };
                    let _ = tx.send(TaskMsg::Vm(format!("Compilation failed — {}{}\nAborting simulation.\n", loc, diag.message)));
                    let _ = tx.send(TaskMsg::Done);
                    return;
                }
            };
            if let Err(e) = std::fs::write(&out_hex, &artifact.hex) {
                let _ = tx.send(TaskMsg::Vm(format!("Failed to write HEX: {}\n", e)));
                let _ = tx.send(TaskMsg::Done);
                return;
            }
            let _ = tx.send(TaskMsg::Stats(Ok(stats_from(&artifact))));

            let target = artifact.device.clone();
            let _ = tx.send(TaskMsg::Vm(format!("Using target: {}\n", target)));

            let mut cmd = Command::new(PathBuf::from(&vm_path));
            cmd.arg(&out_hex)
               .arg(format!("-mmcu={}", target))
               .arg("-n")
               .arg(max_cycles.to_string())
               .arg("-t")
               .arg("-d");

            match cmd.output() {
                Ok(output) => {
                    let _ = tx.send(TaskMsg::Vm(String::from_utf8_lossy(&output.stdout).into_owned()));
                    let _ = tx.send(TaskMsg::Vm(String::from_utf8_lossy(&output.stderr).into_owned()));
                    let _ = tx.send(TaskMsg::Vm("\nSimulation complete.\n".to_string()));
                }
                Err(e) => {
                    let _ = tx.send(TaskMsg::Vm(format!("Failed to run VM: {}\n", e)));
                }
            }
        } else {
            let _ = tx.send(TaskMsg::Vm("No file selected to simulate.\n".to_string()));
        }
        let _ = tx.send(TaskMsg::Done);
    });
}

pub fn spawn_upload(workspace_dir: Option<PathBuf>, selected_file: Option<PathBuf>, avrdude_path: String, target: String, programmer: String, port: String, baudrate: String, additional_flags: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        if let Some(path) = selected_file {
            let out_hex = out_hex_path(&workspace_dir, &path);
            let _ = tx.send(TaskMsg::Upload(format!("Uploading {:?} to board...\n", out_hex)));
            
            // avrdude command using the correct target and preferences
            let mut cmd = Command::new(avrdude_path);
            cmd.arg(format!("-p{}", target))
               .arg(format!("-c{}", programmer));
               
            if !port.is_empty() {
                cmd.arg(format!("-P{}", port));
            }
            if !baudrate.is_empty() {
                cmd.arg(format!("-b{}", baudrate));
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
