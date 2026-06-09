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
//   1. applies any pin inputs the UI injected (writing the PINx data byte) and
//      services the UART receive queue,
//   2. steps the core forward by one clock-frame worth of cycles,
//   3. publishes a snapshot of the watched I/O registers + captured serial
//      traffic back to the UI.
//
// GPIO fidelity rests on the VM modelling PIN/DDR/PORT as independent raw I/O
// bytes. Serial peripherals use the VM's `capture_io` hook: bytes the program
// writes to UDR/SPDR/TWDR are captured as `IoEvent`s; the SPI master read-back
// is the host-configured `spi_miso`; UART receive is injected best-effort by
// presenting a byte and pulsing RXC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub use ik8bvm::core::IoEvent;

/// A command from the UI to the running engine.
pub enum LiveCmd {
    /// Force a single pin's input level (writes bit `bit` of the PINx register
    /// at `addr`). This is how a button/switch drives the pin.
    SetInput { addr: u32, bit: u8, high: bool },
    /// Stop forcing a pin, letting the program drive it again.
    ClearInput { addr: u32, bit: u8 },
    /// Queue bytes to deliver to the program's UART receiver.
    UartSend(Vec<u8>),
    /// Set the byte the SPI peripheral returns to the master.
    SetSpiMiso(u8),
    /// Set the analog value (0..1023) on an ADC channel.
    SetAdc { channel: u8, value: u16 },
    /// Tear the engine down.
    Stop,
}

/// A periodic readout sent to the UI each frame.
#[derive(Clone, Debug, Default)]
pub struct LiveSnapshot {
    /// Watched data-space address -> current value.
    pub regs: HashMap<u32, u8>,
    /// Serial-peripheral bytes transmitted since the previous snapshot.
    pub events: Vec<IoEvent>,
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
    pub fn uart_send(&self, bytes: Vec<u8>) {
        let _ = self.cmd_tx.send(LiveCmd::UartSend(bytes));
    }
    pub fn set_spi_miso(&self, byte: u8) {
        let _ = self.cmd_tx.send(LiveCmd::SetSpiMiso(byte));
    }
    pub fn set_adc(&self, channel: u8, value: u16) {
        let _ = self.cmd_tx.send(LiveCmd::SetAdc { channel, value });
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
/// the run to real time (`0` = as fast as possible). `watch` is the set of
/// data-space addresses reported every frame; `spi_miso` is the initial SPI
/// read-back byte.
pub fn spawn(
    device: String,
    hex_path: PathBuf,
    clock_hz: u32,
    watch: Vec<u32>,
    spi_miso: u8,
    responder: Option<Box<dyn ik8bvm::core::BusResponder>>,
    watch_pins: Vec<u32>,
) -> LiveHandle {
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
        vm.capture_io = true;
        vm.spi_miso = spi_miso;
        vm.responder = responder;
        vm.watch_pins = watch_pins.into_iter().collect();

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
                    LiveCmd::UartSend(bytes) => vm.uart_feed(&bytes),
                    LiveCmd::SetSpiMiso(b) => vm.spi_miso = b,
                    LiveCmd::SetAdc { channel, value } => vm.adc_set(channel as usize, value),
                    LiveCmd::Stop => stop = true,
                }
            }
            if stop {
                break;
            }

            // 2. Apply forced inputs, advance attached devices, then step.
            apply_forced(&mut vm, &forced);
            vm.poll_devices(cycles_per_frame.max(1));
            if vm.running {
                let mut steps = 0u64;
                if cycles_per_frame == 0 {
                    while vm.running && steps < MAX_STEPS_PER_FRAME {
                        vm.step();
                        steps += 1;
                    }
                } else {
                    let target = vm.cycles + cycles_per_frame.max(1);
                    while vm.running && vm.cycles < target && steps < MAX_STEPS_PER_FRAME {
                        vm.step();
                        steps += 1;
                    }
                }
                // Re-assert inputs after stepping too, so the snapshot reflects
                // the forced levels even if the program wrote the PINx byte.
                apply_forced(&mut vm, &forced);
            }

            // 3. Publish a snapshot (always while running; once on halt).
            let halted = !vm.running;
            let events = std::mem::take(&mut vm.io_events);
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
                        events,
                        cycles: vm.cycles,
                        running: vm.running,
                        halt_reason,
                    })
                    .is_err()
                {
                    break; // UI dropped the receiver.
                }
            }

            // Pace to ~real time; when halted, idle but stay responsive to Stop.
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
    use ik8bvm::core::IoPeripheral;

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

    /// UART transmissions are captured as events when `capture_io` is on.
    #[test]
    fn uart_transmit_is_captured() {
        let src = "target atmega328p\n\
                   import std/uart\n\
                   @main {\n\
                       @uart_init(103)\n\
                       @uart_print_str(\"Hi\")\n\
                   }\n";
        analysis::sync_std_imports(src);
        let art = analysis::compile(src).expect("program compiles");

        let hex = std::env::temp_dir().join("ikide_uart_test.hex");
        std::fs::write(&hex, &art.hex).unwrap();

        let mut vm = runner::build_vm(&art.device);
        ik8bvm::hw::load_hex(&mut vm, &hex.to_string_lossy()).expect("HEX loads");
        vm.capture_io = true;

        let mut steps = 0;
        while vm.running && steps < 2_000_000 {
            vm.step();
            steps += 1;
        }

        let sent: Vec<u8> = vm
            .io_events
            .iter()
            .filter(|e| e.periph == IoPeripheral::Uart && e.write)
            .map(|e| e.byte)
            .collect();
        assert_eq!(sent, b"Hi", "the UART stream should capture the printed text");
    }

    /// SPI transmissions are captured, and the configured MISO byte is latched
    /// into the data register for the master to read back (SPDR io=0x2e ->
    /// data-space 0x4e on the atmega328p).
    #[test]
    fn spi_transfer_captures_and_returns_miso() {
        let src = "target atmega328p\n\
                   import std/spi\n\
                   @main {\n\
                       @spi_init_master_raw()\n\
                       @spi_transfer(0x42)\n\
                   }\n";
        analysis::sync_std_imports(src);
        let art = analysis::compile(src).expect("program compiles");

        let hex = std::env::temp_dir().join("ikide_spi_test.hex");
        std::fs::write(&hex, &art.hex).unwrap();

        let mut vm = runner::build_vm(&art.device);
        ik8bvm::hw::load_hex(&mut vm, &hex.to_string_lossy()).expect("HEX loads");
        vm.capture_io = true;
        vm.spi_miso = 0xA5;

        let mut steps = 0;
        while vm.running && steps < 1_000_000 {
            vm.step();
            steps += 1;
        }

        let sent: Vec<u8> = vm
            .io_events
            .iter()
            .filter(|e| e.periph == IoPeripheral::Spi && e.write)
            .map(|e| e.byte)
            .collect();
        assert_eq!(sent, vec![0x42], "the SPI master byte should be captured");
        assert_eq!(vm.read_data(0x4e), 0xA5, "the data register should read back the MISO response");
    }

    /// Fed UART bytes are received and echoed back: RXC reads as set while the
    /// queue is non-empty and reading UDR pops one byte.
    #[test]
    fn uart_receive_echoes_fed_bytes() {
        let src = "target atmega328p\n\
                   import std/uart\n\
                   @main {\n\
                       @uart_init(103)\n\
                       loop * {\n\
                           ram mut $c: u8 = 0\n\
                           @uart_receive() -> $c\n\
                           @uart_send($c)\n\
                       }\n\
                   }\n";
        analysis::sync_std_imports(src);
        let art = analysis::compile(src).expect("program compiles");

        let hex = std::env::temp_dir().join("ikide_uartrx_test.hex");
        std::fs::write(&hex, &art.hex).unwrap();

        let mut vm = runner::build_vm(&art.device);
        ik8bvm::hw::load_hex(&mut vm, &hex.to_string_lossy()).expect("HEX loads");
        vm.capture_io = true;
        vm.uart_feed(b"AB");

        let mut steps = 0;
        while vm.running && steps < 1_000_000 {
            vm.step();
            steps += 1;
        }

        let echoed: Vec<u8> = vm
            .io_events
            .iter()
            .filter(|e| e.periph == IoPeripheral::Uart && e.write)
            .map(|e| e.byte)
            .collect();
        assert_eq!(echoed, b"AB", "the program should echo the bytes fed to its receiver");
    }

    /// A mock device on every bus: SPI returns mosi+1; an I2C register file at
    /// address 0x50; a UART byte queued to send to the MCU.
    #[derive(Default)]
    struct MockDevice {
        regs: [u8; 4],
        ptr: usize,
        addressed: bool,
        uart_out: std::collections::VecDeque<u8>,
        uart_in: Vec<u8>,
    }
    impl ik8bvm::core::BusResponder for MockDevice {
        fn spi_transfer(&mut self, mosi: u8) -> u8 {
            mosi.wrapping_add(1)
        }
        fn i2c_address(&mut self, addr: u8, _read: bool) -> bool {
            self.addressed = addr == 0x50;
            self.addressed
        }
        fn i2c_write(&mut self, byte: u8) -> bool {
            self.ptr = (byte as usize) % self.regs.len();
            true
        }
        fn i2c_read(&mut self, _last: bool) -> u8 {
            let b = self.regs[self.ptr];
            self.ptr = (self.ptr + 1) % self.regs.len();
            b
        }
        fn uart_tx(&mut self, byte: u8) {
            self.uart_in.push(byte);
        }
        fn uart_poll(&mut self) -> Option<u8> {
            self.uart_out.pop_front()
        }
    }

    /// SPI routes each transferred byte through the device's `spi_transfer`.
    #[test]
    fn spi_routes_through_responder() {
        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(MockDevice::default()));
        vm.write_data(0x4E, 0x10); // SPDR (io 0x2E -> data-space 0x4E)
        assert_eq!(vm.read_data(0x4E), 0x11, "MISO should be the device's mosi+1");
    }

    /// A full I2C master sequence (START, addr+W, set pointer, repeated START,
    /// addr+R, read) reaches the device and reads back its register value.
    #[test]
    fn i2c_master_sequence_drives_device() {
        let mut dev = MockDevice::default();
        dev.regs = [0xAA, 0xBB, 0xCC, 0xDD];
        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(dev));

        // atmega328p: TWDR data-space 0xBB, TWCR data-space 0xBC.
        const TWDR: u32 = 0xBB;
        const TWCR: u32 = 0xBC;
        const START: u8 = 0xA4; // TWINT|TWSTA|TWEN
        const EN: u8 = 0x84; // TWINT|TWEN
        const STOP: u8 = 0x94; // TWINT|TWSTO|TWEN

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0); // address 0x50 + write
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x01); // set register pointer = 1
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, START); // repeated start
        vm.write_data(TWDR, 0xA1); // address 0x50 + read
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN); // read with NACK (last byte)
        let got = vm.read_data(TWDR);
        vm.write_data(TWCR, STOP);

        assert_eq!(got, 0xBB, "the master should read register 1's value");
    }

    /// A device's queued UART byte is delivered to the MCU via poll_devices:
    /// RXC reads set and the data register yields the byte.
    #[test]
    fn uart_device_byte_reaches_mcu() {
        let mut dev = MockDevice::default();
        dev.uart_out.push_back(b'Z');
        let mut vm = runner::build_vm("atmega328p");
        vm.responder = Some(Box::new(dev));

        vm.poll_devices(0);
        assert!(vm.read_data(0xC0) & 0x80 != 0, "RXC should be set"); // UCSR0A
        assert_eq!(vm.read_data(0xC6), b'Z'); // UDR0
    }

    /// An ADC conversion latches the host-supplied channel value into ADCH:ADCL
    /// and clears ADSC (atmega328p: ADMUX=0x7C, ADCSRA=0x7A, ADCL=0x78, ADCH=0x79).
    #[test]
    fn adc_conversion_latches_injected_value() {
        let mut vm = runner::build_vm("atmega328p");
        vm.adc_set(2, 0x2AB); // 683, right-adjusted

        vm.write_data(0x7C, 0x02); // ADMUX: select channel 2
        vm.write_data(0x7A, 0xC7); // ADCSRA: ADEN | ADSC | prescaler

        assert_eq!(vm.read_data(0x7A) & 0x40, 0, "ADSC should be cleared after conversion");
        let lo = vm.read_data(0x78) as u16;
        let hi = vm.read_data(0x79) as u16;
        assert_eq!(lo | (hi << 8), 0x2AB, "ADCH:ADCL should hold the injected value");
    }
}
