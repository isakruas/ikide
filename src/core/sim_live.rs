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

// Live, interactive simulation engine for the breadboard.
//
// Unlike `runner::spawn_simulate` (a one-shot run that reports a final
// snapshot), this drives the same ik8bvm core *continuously* on a background
// thread, paced to wall-clock time by an assumed CPU clock. Each frame it:
//   1. applies any pin inputs the UI injected (writing the PINx data byte),
//   2. steps the core forward by one clock-frame worth of cycles,
//   3. publishes a snapshot of the watched I/O registers back to the UI.
//
// The fidelity rests on the VM modelling PIN/DDR/PORT as independent raw I/O
// bytes (no PORT->PIN copy, no toggle-on-write): outputs are read straight
// from PORTx (gated by DDRx), and an external input is just the PINx byte we
// write — exactly how a button wired to the pin would read.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// A command from the UI to the running engine.
pub enum LiveCmd {
    /// Force a single pin's input level (writes bit `bit` of the PINx register
    /// at `addr`). This is how a button/switch drives the pin.
    SetInput { addr: u32, bit: u8, high: bool },
    /// Stop forcing a pin, letting the program drive it again.
    ClearInput { addr: u32, bit: u8 },
    /// Tear the engine down.
    Stop,
}

/// A periodic readout of the watched registers, sent to the UI each frame.
#[derive(Clone, Debug, Default)]
pub struct LiveSnapshot {
    /// Watched data-space address -> current value.
    pub regs: HashMap<u32, u8>,
    pub cycles: u64,
    pub running: bool,
    /// Set once when the core halts, so the UI can report why.
    pub halt_reason: Option<String>,
}

/// Handle the UI keeps while a live simulation is running.
pub struct LiveHandle {
    pub cmd_tx: Sender<LiveCmd>,
    pub snap_rx: Receiver<LiveSnapshot>,
}

impl LiveHandle {
    pub fn set_input(&self, addr: u32, bit: u8, high: bool) {
        let _ = self.cmd_tx.send(LiveCmd::SetInput { addr, bit, high });
    }
    pub fn clear_input(&self, addr: u32, bit: u8) {
        let _ = self.cmd_tx.send(LiveCmd::ClearInput { addr, bit });
    }
    pub fn stop(&self) {
        let _ = self.cmd_tx.send(LiveCmd::Stop);
    }
}

/// Repaint cadence: one engine frame ~= 60 Hz.
const FRAME: Duration = Duration::from_millis(16);
/// Safety cap on instructions per frame so a tight loop with a huge assumed
/// clock can't wedge the thread (still far above any real per-frame budget at
/// sane clock rates).
const MAX_STEPS_PER_FRAME: u64 = 8_000_000;

/// Spawn the live engine. `clock_hz` is the assumed CPU frequency used to pace
/// the run to real time (so cycle-counted `delay_*` loops play at the rate the
/// code intends); `0` means run as fast as possible. `watch` is the set of
/// data-space addresses whose values are reported every frame.
pub fn spawn(device: String, hex_path: PathBuf, clock_hz: u32, watch: Vec<u32>) -> LiveHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<LiveCmd>();
    let (snap_tx, snap_rx) = mpsc::channel::<LiveSnapshot>();

    thread::spawn(move || {
        let mut vm = crate::core::runner::build_vm(&device);
        if ik8bvm::hw::load_hex(&mut vm, &hex_path.to_string_lossy()).is_err() {
            let _ = snap_tx.send(LiveSnapshot {
                running: false,
                halt_reason: Some("failed to load HEX into the simulator".to_string()),
                ..Default::default()
            });
            return;
        }

        // Pins the UI is currently forcing as inputs, re-asserted every frame so
        // a held button stays held.
        let mut forced: HashMap<(u32, u8), bool> = HashMap::new();
        let mut halted_sent = false;

        // Cycles to advance per frame to track real time at the assumed clock.
        let cycles_per_frame: u64 = if clock_hz == 0 {
            0
        } else {
            ((clock_hz as f64) * FRAME.as_secs_f64()) as u64
        };

        loop {
            let frame_start = Instant::now();

            // 1. Drain UI commands.
            let mut stop = false;
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    LiveCmd::SetInput { addr, bit, high } => {
                        forced.insert((addr, bit), high);
                    }
                    LiveCmd::ClearInput { addr, bit } => {
                        forced.remove(&(addr, bit));
                    }
                    LiveCmd::Stop => stop = true,
                }
            }
            if stop {
                break;
            }

            // 2. Apply forced inputs, then step a frame's worth of cycles.
            apply_forced(&mut vm, &forced);
            if vm.running {
                let target = vm.cycles + cycles_per_frame.max(1);
                let mut steps = 0u64;
                if cycles_per_frame == 0 {
                    // Unlimited: run a fat fixed batch, still bounded.
                    while vm.running && steps < MAX_STEPS_PER_FRAME {
                        vm.step();
                        steps += 1;
                    }
                } else {
                    while vm.running && vm.cycles < target && steps < MAX_STEPS_PER_FRAME {
                        vm.step();
                        steps += 1;
                    }
                }
                // Re-assert inputs after stepping too, so the snapshot the UI
                // sees reflects the forced levels even if the program briefly
                // wrote the PINx byte.
                apply_forced(&mut vm, &forced);
            }

            // 3. Publish a snapshot (always while running; once on halt).
            let halted = !vm.running;
            if !halted || !halted_sent {
                let mut regs = HashMap::with_capacity(watch.len());
                for &a in &watch {
                    regs.insert(a, vm.read_data(a));
                }
                let halt_reason = if halted {
                    halted_sent = true;
                    Some(if vm.unknown_opcode {
                        format!("Halted on unknown opcode at PC=0x{:06X}", vm.pc)
                    } else {
                        "Program halted (sleep / RJMP .-2)".to_string()
                    })
                } else {
                    None
                };
                if snap_tx
                    .send(LiveSnapshot {
                        regs,
                        cycles: vm.cycles,
                        running: vm.running,
                        halt_reason,
                    })
                    .is_err()
                {
                    // UI dropped the receiver; nothing left to drive.
                    break;
                }
            }

            // Pace to ~real time: sleep out the rest of the frame. When halted,
            // idle but stay responsive to a Stop command.
            if let Some(rem) = FRAME.checked_sub(frame_start.elapsed()) {
                thread::sleep(rem);
            }
        }
    });

    LiveHandle { cmd_tx, snap_rx }
}

/// Write every forced pin level into its PINx data byte.
fn apply_forced(vm: &mut ik8bvm::core::AvrVm, forced: &HashMap<(u32, u8), bool>) {
    for (&(addr, bit), &high) in forced {
        let mut v = vm.read_data(addr);
        if high {
            v |= 1 << bit;
        } else {
            v &= !(1 << bit);
        }
        vm.write_data(addr, v);
    }
}

#[cfg(test)]
mod tests {
    use crate::core::{analysis, runner};

    /// End-to-end check of the path the breadboard's LED indicator relies on:
    /// a real GPIO program drives PB5 high, and the watched PORTB/DDRB bits in
    /// the simulated core reflect it (PORTB=0x25, DDRB=0x24 on the atmega328p).
    #[test]
    fn output_pin_drives_port_bit_high() {
        let src = "target atmega328p\n\
                   import std/gpio\n\
                   @main {\n\
                       @pin_mode_b(5, 1)\n\
                       @digital_write_b(5, 1)\n\
                   }\n";
        analysis::sync_std_imports(src);
        let art = analysis::compile(src).expect("program compiles");
        assert_eq!(art.device, "atmega328p");

        let hex = std::env::temp_dir().join("ikide_simlive_test.hex");
        std::fs::write(&hex, &art.hex).unwrap();

        let mut vm = runner::build_vm(&art.device);
        ik8bvm::hw::load_hex(&mut vm, &hex.to_string_lossy()).expect("HEX loads");

        let mut steps = 0;
        while vm.running && steps < 200_000 {
            vm.step();
            steps += 1;
        }

        let ddr = vm.read_data(0x24);
        let port = vm.read_data(0x25);
        assert_eq!((ddr >> 5) & 1, 1, "DDRB bit 5 should be configured as output");
        assert_eq!((port >> 5) & 1, 1, "PORTB bit 5 should be driven high");
    }
}
