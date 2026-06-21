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

//! A general-purpose, Rhai-scripted test harness for ik programs.
//!
//! The IDE already drives the ik8bvm core live on the breadboard
//! (`sim_live.rs`): it injects UART/SPI/ADC/pin events and reads back what the
//! program does. This module repackages that same machinery as a *headless,
//! scriptable* bench so a user can write `<workspace>/tests/*.rhai` files and
//! assert on anything the program does — it is deliberately NOT specialised for
//! any one program (no shell prompts, no kernel assumptions). A test file gets
//! full control of the core through a small set of registered functions:
//!
//! * load a program: `build("prog.ik")` (compile in-process) or `load_hex(...)`;
//! * attach the same `assets/devices/*.rhai` models the breadboard uses with
//!   `load_device(...)`;
//! * advance: `step(n)`, `step_until_uart(text, max)`, `reset`;
//! * assert: `check`, `check_eq`, `check_contains`, `assert`, `log`.
//!
//! Every peripheral is controllable from *both* sides — what the program reads
//! and what it writes:
//!
//! | bus    | drive (into the MCU)   | observe (out of the MCU)        |
//! |--------|------------------------|---------------------------------|
//! | UART   | `uart_send`/`uart_byte`| `uart_recv`/`uart_text`         |
//! | SPI    | `set_spi_miso` + device| `spi_mosi` / `spi_miso_in`      |
//! | I2C/TWI| `load_device`          | `twi_data`                      |
//! | ADC    | `set_adc`              | `peek`(ADC regs)                |
//! | GPIO   | `set_pin`/`clear_pin`  | `pin` / `peek`(PORT)            |
//! | SRAM/IO| `poke` / `set_reg`     | `peek` / `reg`                  |
//! | EEPROM | `eeprom_write`         | `eeprom_read`                   |
//! | core   | —                      | `pc`/`sp`/`sreg`/`cycles`       |
//!
//! Each `.rhai` file runs against a fresh [`Bench`]; the recorded checks are
//! streamed back to the IDE as a PASS/FAIL report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use ik8bvm::core::{AvrVm, IoKind, IoPeripheral};
use rhai::{Array, Dynamic, Engine, EvalAltResult, ImmutableString};

use crate::core::analysis;
use crate::core::devices::{self, DeviceBus, ScriptedDevice, SharedBus};
use crate::core::runner::{build_vm, TaskMsg};

/// Step in chunks so attached devices get polled periodically during long runs.
const PUMP_CHUNK: u64 = 8_192;

/// One recorded assertion outcome.
struct CheckResult {
    name: String,
    ok: bool,
    detail: String,
}

/// A single test's view of the simulated machine: the core, the attached device
/// bus, the forced input pins, and the captured UART output, plus the running
/// tally of checks the script has made.
pub struct Bench {
    workspace: Option<PathBuf>,
    target: String,
    hex_loaded: bool,
    vm: AvrVm,
    bus: Option<Arc<Mutex<DeviceBus>>>,
    /// Rhai engine used to compile attached device scripts (the breadboard's).
    dev_engine: Arc<Engine>,
    /// Pin levels the test (or a device) holds as inputs, re-asserted each pump.
    forced: HashMap<(u32, u8), bool>,
    /// All UART bytes the program has transmitted (accumulated as text).
    uart_buf: String,
    /// UART bytes not yet consumed by `uart_recv`.
    uart_unread: String,
    /// SPI bytes the MCU shifted *out* (MOSI), not yet consumed by `spi_mosi`.
    spi_mosi: Vec<u8>,
    /// SPI bytes the MCU shifted *in* (MISO), not yet consumed by `spi_miso_in`.
    spi_miso_in: Vec<u8>,
    /// I2C/TWI data bytes seen on the bus, not yet consumed by `twi_data`.
    twi_data: Vec<u8>,
    executed: u64,
    results: Vec<CheckResult>,
    logs: Vec<String>,
}

impl Bench {
    fn new(workspace: Option<PathBuf>, target: String) -> Self {
        let mut vm = build_vm(&target);
        vm.capture_io = true;
        Bench {
            workspace,
            target,
            hex_loaded: false,
            vm,
            bus: None,
            dev_engine: devices::engine(),
            forced: HashMap::new(),
            uart_buf: String::new(),
            uart_unread: String::new(),
            spi_mosi: Vec::new(),
            spi_miso_in: Vec::new(),
            twi_data: Vec::new(),
            executed: 0,
            results: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// Resolve a test-supplied path against the workspace when it is relative.
    fn resolve(&self, p: &str) -> PathBuf {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            pb
        } else if let Some(ws) = &self.workspace {
            ws.join(pb)
        } else {
            pb
        }
    }

    /// Re-point the (possibly rebuilt) core at the attached device bus.
    fn reattach_bus(&mut self) {
        if let Some(bus) = &self.bus {
            let b = bus.lock().unwrap();
            self.vm.watch_pins = b.pin_addrs().into_iter().collect();
            for (addr, bit, level) in b.idle_drives() {
                self.forced.insert((addr, bit), level);
            }
            drop(b);
            self.vm.responder = Some(Box::new(SharedBus(bus.clone())));
        }
    }

    /// Build a fresh core for the current target and load `hex_file` into it,
    /// keeping the attached bus and forced pins.
    fn load_program(&mut self, hex_file: &str) -> Result<(), String> {
        let mut vm = build_vm(&self.target);
        vm.capture_io = true;
        ik8bvm::hw::load_hex(&mut vm, hex_file).map_err(|e| e.to_string())?;
        self.vm = vm;
        self.clear_io();
        self.executed = 0;
        self.hex_loaded = true;
        self.reattach_bus();
        Ok(())
    }

    /// Compile an ik source file in-process and load it. The target is taken
    /// from the program's own `target` directive.
    fn build(&mut self, rel: &str) -> Result<(), String> {
        let path = self.resolve(rel);
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", rel, e))?;
        analysis::sync_std_imports(&src);
        // Local `import foo` resolves relative to the current directory, so for a
        // multi-file program compile from the source file's own directory (then
        // restore), letting sibling modules be found. The chdir is process-global,
        // so serialize it behind a lock to stay correct under concurrent builds.
        static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let compiled = {
            let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev_cwd = std::env::current_dir().ok();
            if let Some(dir) = path.parent() {
                if dir.as_os_str().len() > 0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
            let r = analysis::compile(&src);
            if let Some(p) = prev_cwd {
                let _ = std::env::set_current_dir(p);
            }
            r
        };
        let artifact = compiled.map_err(|d| {
            let loc = if d.line > 0 { format!("line {}: ", d.line) } else { String::new() };
            format!("compiling {}: {}{}", rel, loc, d.message)
        })?;
        self.target = artifact.device.clone();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("test");
        let hexf = std::env::temp_dir().join(format!("ikide_test_{}.hex", stem));
        std::fs::write(&hexf, &artifact.hex).map_err(|e| format!("writing hex: {}", e))?;
        self.load_program(&hexf.to_string_lossy())
    }

    /// Load a prebuilt Intel-HEX image.
    fn load_hex(&mut self, rel: &str) -> Result<(), String> {
        let path = self.resolve(rel);
        self.load_program(&path.to_string_lossy())
    }

    /// Attach one of the breadboard device models to the bus.
    /// Attach a device by built-in name (e.g. `load_device("at24c256")`, resolved
    /// from the shipped catalog like the breadboard) or by a path to a `.rhai`
    /// file the user dropped in the workspace.
    fn load_device(&mut self, name_or_path: &str) -> Result<(), String> {
        let src = if let Some(s) = devices::builtin_device_src(name_or_path) {
            s.to_string()
        } else {
            let path = self.resolve(name_or_path);
            std::fs::read_to_string(&path).map_err(|e| {
                format!("device '{}' is not a built-in name and could not be read as a file: {}", name_or_path, e)
            })?
        };
        let wiring: HashMap<String, String> = HashMap::new();
        let dev = ScriptedDevice::from_src(self.dev_engine.clone(), &src, &self.target, &wiring)?;
        if self.bus.is_none() {
            self.bus = Some(Arc::new(Mutex::new(DeviceBus::default())));
        }
        self.bus.as_ref().unwrap().lock().unwrap().add(dev);
        self.reattach_bus();
        Ok(())
    }

    /// Write every held input level into its PINx data byte.
    fn apply_forced(&mut self) {
        let pairs: Vec<((u32, u8), bool)> = self.forced.iter().map(|(&k, &v)| (k, v)).collect();
        for ((addr, bit), high) in pairs {
            let mut v = self.vm.read_data(addr);
            if high {
                v |= 1 << bit;
            } else {
                v &= !(1 << bit);
            }
            self.vm.write_data(addr, v);
        }
    }

    /// Forget all captured bus traffic (on a fresh program / reset).
    fn clear_io(&mut self) {
        self.uart_buf.clear();
        self.uart_unread.clear();
        self.spi_mosi.clear();
        self.spi_miso_in.clear();
        self.twi_data.clear();
    }

    /// Route the bus traffic the program produced into the per-bus buffers, so a
    /// test can read *both* directions (what the MCU sent and what it received).
    fn drain_io(&mut self) {
        let events = std::mem::take(&mut self.vm.io_events);
        for e in events {
            match e.periph {
                IoPeripheral::Uart if e.write => {
                    // MCU transmitted a byte (its console/serial output).
                    self.uart_buf.push(e.byte as char);
                    self.uart_unread.push(e.byte as char);
                }
                IoPeripheral::Uart => {} // MCU read an input byte we fed; nothing to capture.
                IoPeripheral::Spi if e.write => self.spi_mosi.push(e.byte), // MOSI: MCU -> device.
                IoPeripheral::Spi => self.spi_miso_in.push(e.byte),         // MISO: device -> MCU.
                IoPeripheral::Twi => {
                    if e.kind == IoKind::Data {
                        self.twi_data.push(e.byte);
                    }
                }
            }
        }
    }

    /// Advance up to `max_steps` instructions, pumping devices and forced pins.
    fn pump(&mut self, max_steps: u64) {
        if let Some(bus) = &self.bus {
            for (addr, bit, level) in bus.lock().unwrap().take_drives() {
                self.forced.insert((addr, bit), level);
            }
        }
        self.apply_forced();
        self.vm.poll_devices(max_steps.max(1));
        let mut steps = 0u64;
        while self.vm.running && steps < max_steps {
            self.vm.step();
            steps += 1;
        }
        self.apply_forced();
        self.executed += steps;
        self.drain_io();
    }

    /// Run `n` instructions (in pump-sized chunks), stopping early on halt.
    fn step(&mut self, n: u64) {
        let mut left = n;
        while left > 0 && self.vm.running {
            let chunk = left.min(PUMP_CHUNK);
            self.pump(chunk);
            left -= chunk;
        }
    }

    /// Step until the accumulated UART output contains `needle` or the budget is
    /// spent / the core halts. Returns whether it was found.
    fn step_until_uart(&mut self, needle: &str, max: u64) -> bool {
        if self.uart_buf.contains(needle) {
            return true;
        }
        let mut spent = 0u64;
        while spent < max && self.vm.running {
            let chunk = (max - spent).min(PUMP_CHUNK);
            self.pump(chunk);
            spent += chunk;
            if self.uart_buf.contains(needle) {
                return true;
            }
        }
        self.uart_buf.contains(needle)
    }

    fn record(&mut self, name: &str, ok: bool, detail: String) -> bool {
        self.results.push(CheckResult { name: name.to_string(), ok, detail });
        ok
    }
}

/// Map a Rust error string into a Rhai runtime error.
fn rt(e: String) -> Box<EvalAltResult> {
    e.into()
}

/// Build a Rhai engine whose functions all act on the shared [`Bench`].
fn build_engine(bench: Arc<Mutex<Bench>>) -> Engine {
    let mut e = Engine::new();
    e.set_max_operations(50_000_000);
    e.set_max_expr_depths(128, 128);

    macro_rules! reg {
        ($name:expr, $f:expr) => {{
            let b = bench.clone();
            e.register_fn($name, $f(b));
        }};
    }

    // ---- program loading ------------------------------------------------
    reg!("build", |b: Arc<Mutex<Bench>>| {
        move |path: &str| -> Result<(), Box<EvalAltResult>> { b.lock().unwrap().build(path).map_err(rt) }
    });
    reg!("load_hex", |b: Arc<Mutex<Bench>>| {
        move |path: &str| -> Result<(), Box<EvalAltResult>> { b.lock().unwrap().load_hex(path).map_err(rt) }
    });
    reg!("load_device", |b: Arc<Mutex<Bench>>| {
        move |path: &str| -> Result<(), Box<EvalAltResult>> { b.lock().unwrap().load_device(path).map_err(rt) }
    });
    reg!("reset", |b: Arc<Mutex<Bench>>| {
        move || {
            let mut g = b.lock().unwrap();
            g.vm.reset();
            g.clear_io();
            g.executed = 0;
            g.apply_forced();
        }
    });

    // ---- execution ------------------------------------------------------
    reg!("step", |b: Arc<Mutex<Bench>>| {
        move |n: i64| b.lock().unwrap().step(n.max(0) as u64)
    });
    reg!("step", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().step(1)
    });
    reg!("step_until_uart", |b: Arc<Mutex<Bench>>| {
        move |needle: &str, max: i64| b.lock().unwrap().step_until_uart(needle, max.max(0) as u64)
    });
    reg!("running", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().vm.running
    });
    reg!("cycles", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().vm.cycles as i64
    });
    reg!("executed", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().executed as i64
    });

    // ---- injection ------------------------------------------------------
    reg!("uart_send", |b: Arc<Mutex<Bench>>| {
        move |s: &str| b.lock().unwrap().vm.uart_feed(s.as_bytes())
    });
    reg!("uart_byte", |b: Arc<Mutex<Bench>>| {
        move |byte: i64| b.lock().unwrap().vm.uart_feed(&[byte as u8])
    });
    reg!("set_adc", |b: Arc<Mutex<Bench>>| {
        move |ch: i64, v: i64| b.lock().unwrap().vm.adc_set(ch.max(0) as usize, v.clamp(0, 0xFFFF) as u16)
    });
    reg!("set_spi_miso", |b: Arc<Mutex<Bench>>| {
        move |byte: i64| b.lock().unwrap().vm.spi_miso = byte as u8
    });
    reg!("eeprom_write", |b: Arc<Mutex<Bench>>| {
        move |addr: i64, v: i64| {
            let mut g = b.lock().unwrap();
            let a = addr as usize;
            if a < g.vm.eeprom.len() {
                g.vm.eeprom[a] = v as u8;
            }
        }
    });
    reg!("set_pin", |b: Arc<Mutex<Bench>>| {
        move |addr: i64, bit: i64, high: bool| {
            b.lock().unwrap().forced.insert((addr as u32, bit as u8), high);
        }
    });
    reg!("clear_pin", |b: Arc<Mutex<Bench>>| {
        move |addr: i64, bit: i64| {
            b.lock().unwrap().forced.remove(&(addr as u32, bit as u8));
        }
    });
    reg!("poke", |b: Arc<Mutex<Bench>>| {
        move |addr: i64, v: i64| b.lock().unwrap().vm.write_data(addr as u32, v as u8)
    });
    reg!("set_reg", |b: Arc<Mutex<Bench>>| {
        move |i: i64, v: i64| {
            let mut g = b.lock().unwrap();
            if (0..32).contains(&i) {
                g.vm.r[i as usize] = v as u8;
            }
        }
    });
    // Raise any interrupt vector by index (e.g. simulate a source the VM does
    // not auto-generate); serviced on the next step if the I flag is set.
    reg!("irq", |b: Arc<Mutex<Bench>>| {
        move |vector: i64| {
            if (0..256).contains(&vector) {
                b.lock().unwrap().vm.raise_interrupt(vector as u8);
            }
        }
    });

    // ---- observation ----------------------------------------------------
    reg!("peek", |b: Arc<Mutex<Bench>>| {
        move |addr: i64| b.lock().unwrap().vm.read_data(addr as u32) as i64
    });
    reg!("pin", |b: Arc<Mutex<Bench>>| {
        move |addr: i64, bit: i64| -> bool {
            let mut g = b.lock().unwrap();
            (g.vm.read_data(addr as u32) >> (bit as u8 & 7)) & 1 == 1
        }
    });
    reg!("eeprom_read", |b: Arc<Mutex<Bench>>| {
        move |addr: i64| -> i64 {
            let g = b.lock().unwrap();
            let a = addr as usize;
            if a < g.vm.eeprom.len() { g.vm.eeprom[a] as i64 } else { 0 }
        }
    });
    reg!("spi_mosi", |b: Arc<Mutex<Bench>>| {
        move || -> Array {
            let mut g = b.lock().unwrap();
            std::mem::take(&mut g.spi_mosi).into_iter().map(|x| Dynamic::from(x as i64)).collect()
        }
    });
    reg!("spi_miso_in", |b: Arc<Mutex<Bench>>| {
        move || -> Array {
            let mut g = b.lock().unwrap();
            std::mem::take(&mut g.spi_miso_in).into_iter().map(|x| Dynamic::from(x as i64)).collect()
        }
    });
    reg!("twi_data", |b: Arc<Mutex<Bench>>| {
        move || -> Array {
            let mut g = b.lock().unwrap();
            std::mem::take(&mut g.twi_data).into_iter().map(|x| Dynamic::from(x as i64)).collect()
        }
    });

    reg!("reg", |b: Arc<Mutex<Bench>>| {
        move |i: i64| -> i64 {
            let g = b.lock().unwrap();
            if (0..32).contains(&i) { g.vm.r[i as usize] as i64 } else { 0 }
        }
    });
    reg!("pc", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().vm.pc as i64
    });
    reg!("sp", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().vm.sp as i64
    });
    reg!("sreg", |b: Arc<Mutex<Bench>>| {
        move || b.lock().unwrap().vm.sreg as i64
    });
    reg!("uart_recv", |b: Arc<Mutex<Bench>>| {
        move || -> ImmutableString {
            let mut g = b.lock().unwrap();
            let s = std::mem::take(&mut g.uart_unread);
            s.into()
        }
    });
    reg!("uart_text", |b: Arc<Mutex<Bench>>| {
        move || -> ImmutableString { b.lock().unwrap().uart_buf.clone().into() }
    });
    reg!("uart_clear", |b: Arc<Mutex<Bench>>| {
        move || {
            let mut g = b.lock().unwrap();
            g.uart_buf.clear();
            g.uart_unread.clear();
        }
    });

    // ---- assertions / reporting -----------------------------------------
    reg!("check", |b: Arc<Mutex<Bench>>| {
        move |name: &str, cond: bool| {
            let detail = if cond { String::new() } else { "condition was false".to_string() };
            b.lock().unwrap().record(name, cond, detail)
        }
    });
    reg!("check_eq", |b: Arc<Mutex<Bench>>| {
        move |name: &str, got: i64, want: i64| {
            let ok = got == want;
            let detail = if ok { String::new() } else { format!("(expected {}, got {})", want, got) };
            b.lock().unwrap().record(name, ok, detail)
        }
    });
    reg!("check_contains", |b: Arc<Mutex<Bench>>| {
        move |name: &str, hay: &str, needle: &str| {
            let ok = hay.contains(needle);
            let detail = if ok { String::new() } else { format!("(missing {:?})", needle) };
            b.lock().unwrap().record(name, ok, detail)
        }
    });
    reg!("assert", |_b: Arc<Mutex<Bench>>| {
        move |cond: bool, msg: &str| -> Result<(), Box<EvalAltResult>> {
            if cond { Ok(()) } else { Err(rt(format!("assertion failed: {}", msg))) }
        }
    });
    reg!("log", |b: Arc<Mutex<Bench>>| {
        move |msg: &str| b.lock().unwrap().logs.push(msg.to_string())
    });

    e
}

/// Discover and run every `<workspace>/tests/*.rhai` file, streaming a PASS/FAIL
/// report back to the IDE. Each file runs against a fresh [`Bench`].
pub fn spawn_run_tests(workspace: Option<PathBuf>, target: String, tx: Sender<TaskMsg>) {
    thread::spawn(move || {
        let tests_dir = match &workspace {
            Some(w) => w.join("tests"),
            None => PathBuf::from("tests"),
        };

        let mut files: Vec<PathBuf> = match std::fs::read_dir(&tests_dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("rhai"))
                .collect(),
            Err(_) => {
                let _ = tx.send(TaskMsg::Test(format!(
                    "No tests/ directory found at {:?}.\nCreate one with .rhai test files.\n",
                    tests_dir
                )));
                let _ = tx.send(TaskMsg::Done);
                return;
            }
        };
        files.sort();

        if files.is_empty() {
            let _ = tx.send(TaskMsg::Test(format!("No .rhai test files in {:?}.\n", tests_dir)));
            let _ = tx.send(TaskMsg::Done);
            return;
        }

        let mut total_pass = 0usize;
        let mut total_fail = 0usize;
        let mut files_with_errors = 0usize;

        for file in &files {
            let name = file.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
            let _ = tx.send(TaskMsg::Test(format!("\n=== {} ===\n", name)));

            let src = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    files_with_errors += 1;
                    let _ = tx.send(TaskMsg::Test(format!("  could not read file: {}\n", e)));
                    continue;
                }
            };

            let bench = Arc::new(Mutex::new(Bench::new(workspace.clone(), target.clone())));
            let engine = build_engine(bench.clone());

            let run_err = engine.run(&src).err();

            let g = bench.lock().unwrap();
            for log in &g.logs {
                let _ = tx.send(TaskMsg::Test(format!("  · {}\n", log)));
            }
            for r in &g.results {
                if r.ok {
                    total_pass += 1;
                    let _ = tx.send(TaskMsg::Test(format!("  PASS  {}\n", r.name)));
                } else {
                    total_fail += 1;
                    let detail = if r.detail.is_empty() { String::new() } else { format!("  {}", r.detail) };
                    let _ = tx.send(TaskMsg::Test(format!("  FAIL  {}{}\n", r.name, detail)));
                }
            }
            if !g.hex_loaded && g.results.is_empty() && run_err.is_none() {
                let _ = tx.send(TaskMsg::Test(
                    "  (no program loaded and no checks — call build(\"...\") or load_hex(\"...\"))\n".to_string(),
                ));
            }
            if let Some(err) = run_err {
                files_with_errors += 1;
                let _ = tx.send(TaskMsg::Test(format!("  script error: {}\n", err)));
            }
        }

        let _ = tx.send(TaskMsg::Test(format!(
            "\n{} passed, {} failed, {} total across {} file(s){}\n",
            total_pass,
            total_fail,
            total_pass + total_fail,
            files.len(),
            if files_with_errors > 0 {
                format!(", {} file(s) with errors", files_with_errors)
            } else {
                String::new()
            },
        )));
        let _ = tx.send(TaskMsg::Done);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: compile a tiny ik program in-process, run it on the bench,
    /// and read back what it printed over the UART — exercising the whole
    /// build → step → drain → observe path the Rhai API exposes.
    #[test]
    fn builds_runs_and_captures_uart() {
        let dir = std::env::temp_dir().join("ikide_testbed_it");
        std::fs::create_dir_all(&dir).unwrap();
        let prog = dir.join("hello.ik");
        std::fs::write(
            &prog,
            "target atmega328p\nimport std/uart\n@main {\n    @uart_init(103)\n    @uart_print_str(\"OK\")\n}\n",
        )
        .unwrap();

        let mut b = Bench::new(Some(dir.clone()), "atmega328p".to_string());
        b.build("hello.ik").expect("the tiny program should compile and load");
        assert!(b.hex_loaded);
        b.step(2_000_000);
        assert!(b.uart_buf.contains("OK"), "expected the UART output to contain \"OK\", got {:?}", b.uart_buf);

        // uart_recv drains; a second call sees nothing new.
        assert!(b.uart_unread.contains("OK"));
        let _ = std::mem::take(&mut b.uart_unread);
        assert!(b.uart_unread.is_empty());
    }

    /// Full user-facing path: a `tests/*.rhai` file discovered and run through
    /// the Rhai engine, with its checks reported back over the task channel.
    #[test]
    fn runs_a_rhai_suite_and_reports_pass() {
        let ws = std::env::temp_dir().join("ikide_testbed_suite");
        std::fs::create_dir_all(ws.join("tests")).unwrap();
        std::fs::write(
            ws.join("hello.ik"),
            "target atmega328p\nimport std/uart\n@main {\n    @uart_init(103)\n    @uart_print_str(\"OK\")\n}\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("tests").join("hello.rhai"),
            "build(\"hello.ik\");\nstep(2000000);\ncheck_contains(\"prints OK\", uart_text(), \"OK\");\n",
        )
        .unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        spawn_run_tests(Some(ws), "atmega328p".to_string(), tx);

        let mut report = String::new();
        while let Ok(msg) = rx.recv() {
            match msg {
                TaskMsg::Test(s) => report.push_str(&s),
                TaskMsg::Done => break,
                _ => {}
            }
        }
        assert!(report.contains("PASS  prints OK"), "report was:\n{}", report);
        assert!(report.contains("1 passed, 0 failed"), "report was:\n{}", report);
    }

    /// A USART-RX interrupt handler echoes each received byte + 1 — proves the
    /// VM raises the peripheral (non-timer) interrupt from its RX FIFO event.
    #[test]
    fn uart_rx_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_uart");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/uart\nisr USART_RX {\n    ram imut $c: u8 = %UART0_RX_REG\n    @uart_send($c + 1)\n}\n@main {\n    @uart_init(103)\n    %UART0_CTRLB_REG | 0x80 -> %UART0_CTRLB_REG\n    @sei()\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.vm.uart_feed(b"A");
        b.step(2_000_000);
        assert!(b.uart_buf.contains('B'), "RX ISR should echo 'A'+1='B'; got {:?}", b.uart_buf);
    }

    /// An ADC-complete interrupt handler transmits a byte once the conversion
    /// finishes — proves the ADC completion interrupt path.
    #[test]
    fn adc_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_adc");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/uart\nimport std/adc\nisr ADC {\n    @uart_send(66)\n}\n@main {\n    @uart_init(103)\n    @sei()\n    0xCF -> %ADC_CTRLREG\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.step(2_000_000);
        assert!(b.uart_buf.contains('B'), "ADC ISR should transmit 'B'; got {:?}", b.uart_buf);
    }

    /// SPI transfer-complete interrupt.
    #[test]
    fn spi_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_spi");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/spi\nimport std/uart\nisr SPI_STC {\n    @uart_send(67)\n}\n@main {\n    @uart_init(103)\n    @spi_init_master_raw()\n    %SPI0_CTRL_REG | 0x80 -> %SPI0_CTRL_REG\n    @sei()\n    ram mut $r: u8 = 0\n    @spi_transfer(0x41) -> $r\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.step(2_000_000);
        assert!(b.uart_buf.contains('C'), "SPI ISR should transmit 'C'; got {:?}", b.uart_buf);
    }

    /// TWI operation-complete interrupt.
    #[test]
    fn twi_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_twi");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/twi\nimport std/uart\nisr TWI {\n    @uart_send(68)\n}\n@main {\n    @uart_init(103)\n    @twi_init(72)\n    @sei()\n    0xA5 -> %TWI0_CTRL_REG\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.step(2_000_000);
        assert!(b.uart_buf.contains('D'), "TWI ISR should transmit 'D'; got {:?}", b.uart_buf);
    }

    /// External INT0 on a falling edge of its pin (PD2) runs the handler.
    #[test]
    fn ext_int0_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_int0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/uart\nconst %EICRA: u8 = 0x69\nconst %EIMSK: u8 = 0x3D\nisr INT0 {\n    @uart_send(69)\n}\n@main {\n    @uart_init(103)\n    0x02 -> %EICRA\n    0x01 -> %EIMSK\n    @sei()\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.forced.insert((0x29, 2), true);  // PD2 high (idle, pull-up)
        b.step(200_000);
        b.forced.insert((0x29, 2), false); // falling edge -> INT0
        b.step(500_000);
        assert!(b.uart_buf.contains('E'), "INT0 ISR should transmit 'E'; got {:?}", b.uart_buf);
    }

    /// Pin-change interrupt PCINT0 fires when an enabled pin (PB0) changes.
    #[test]
    fn pcint_interrupt_fires() {
        let dir = std::env::temp_dir().join("ikide_isr_pcint");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.ik"),
            "target atmega328p\nimport std/uart\nconst %PCICR: u8 = 0x68\nconst %PCMSK0: u8 = 0x6B\nisr PCINT0 {\n    @uart_send(70)\n}\n@main {\n    @uart_init(103)\n    0x01 -> %PCMSK0\n    0x01 -> %PCICR\n    @sei()\n    loop * {}\n}\n").unwrap();
        let mut b = Bench::new(Some(dir), "atmega328p".to_string());
        b.build("p.ik").expect("compile");
        b.step(200_000);
        b.forced.insert((0x23, 0), true);  // PB0 high -> pin change
        b.step(500_000);
        assert!(b.uart_buf.contains('F'), "PCINT0 ISR should transmit 'F'; got {:?}", b.uart_buf);
    }

    /// Run every `assets/examples/<name>/tests/*.rhai` suite (slow; `--ignored`).
    #[test]
    #[ignore]
    fn all_example_suites_pass() {
        let root = PathBuf::from("assets/examples");
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten()
            .map(|e| e.path()).filter(|p| p.join("tests").is_dir()).collect();
        dirs.sort();
        let mut failed = Vec::new();
        for dir in dirs {
            let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_run_tests(Some(dir), "atmega328p".to_string(), tx);
            let mut report = String::new();
            while let Ok(msg) = rx.recv() { match msg { TaskMsg::Test(s) => report.push_str(&s), TaskMsg::Done => break, _ => {} } }
            println!("########## {}\n{}", name, report);
            if report.contains("FAIL") || report.contains("script error") || !report.contains("0 failed") {
                failed.push(name);
            }
        }
        assert!(failed.is_empty(), "suites failed: {:?}", failed);
    }
}
