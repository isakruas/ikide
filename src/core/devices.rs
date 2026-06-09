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

// Virtual devices: user-authored peripherals that respond on the MCU's serial
// buses. A device is a small Rhai script exposing a `meta()` descriptor and
// per-event handlers (`spi_transfer`, `i2c_*`, `uart_tx`/`uart_poll`, `tick`).
// `ScriptedDevice` bridges those handlers to the simulator's `BusResponder`
// trait; `DeviceBus` multiplexes several devices onto one bus and is what the
// VM actually drives.
//
// Authoring is just dropping a `.rhai` file — no recompile — so the catalog of
// peripherals can grow without bound.

use std::collections::HashSet;
use std::sync::Arc;

use ik8bvm::core::BusResponder;
use rhai::{AST, Dynamic, Engine, Map, Scope};

/// Which bus a device attaches to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Uart,
    Spi,
    I2c,
}

impl Bus {
    fn parse(s: &str) -> Option<Bus> {
        match s {
            "uart" => Some(Bus::Uart),
            "spi" => Some(Bus::Spi),
            "i2c" | "twi" => Some(Bus::I2c),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Bus::Uart => "UART",
            Bus::Spi => "SPI",
            Bus::I2c => "I2C",
        }
    }
}

/// A device the catalog offers, before instantiation (for the UI picker).
#[derive(Clone, Debug)]
pub struct DeviceSpec {
    pub id: String,
    pub name: String,
    pub bus: Bus,
    pub address: Option<u8>,
    src: String,
}

/// A live device instance backed by a compiled Rhai script. Its mutable state
/// lives in `state` (the script's `this`), persisting across event calls.
pub struct ScriptedDevice {
    engine: Arc<Engine>,
    ast: AST,
    scope: Scope<'static>,
    state: Dynamic,
    fns: HashSet<String>,
    pub name: String,
    pub bus: Bus,
    pub address: Option<u8>,
}

impl ScriptedDevice {
    /// Compile a device script and read its descriptor + initial state.
    pub fn from_src(engine: Arc<Engine>, src: &str) -> Result<Self, String> {
        let ast = engine.compile(src).map_err(|e| e.to_string())?;
        let fns: HashSet<String> = ast.iter_functions().map(|f| f.name.to_string()).collect();
        let mut scope = Scope::new();

        let meta: Map = engine
            .call_fn(&mut scope, &ast, "meta", ())
            .map_err(|e| format!("meta(): {}", e))?;
        let name = meta
            .get("name")
            .and_then(|d| d.clone().into_string().ok())
            .unwrap_or_else(|| "device".to_string());
        let bus = meta
            .get("bus")
            .and_then(|d| d.clone().into_string().ok())
            .and_then(|s| Bus::parse(&s))
            .ok_or_else(|| "meta() must set bus to \"uart\", \"spi\" or \"i2c\"".to_string())?;
        let address = meta.get("address").and_then(|d| d.as_int().ok()).map(|i| i as u8);

        // Initial state: the script's optional init() return, else an empty map.
        let state = if fns.contains("init") {
            engine
                .call_fn::<Dynamic>(&mut scope, &ast, "init", ())
                .map_err(|e| format!("init(): {}", e))?
        } else {
            Dynamic::from(Map::new())
        };

        Ok(ScriptedDevice { engine, ast, scope, state, fns, name, bus, address })
    }

    /// Invoke a script handler with `this` bound to the device state, returning
    /// its result. Missing handlers are a no-op; runtime errors are swallowed
    /// so one bad call can't wedge the simulation.
    fn call(&mut self, name: &str, args: Vec<Dynamic>) -> Option<Dynamic> {
        if !self.fns.contains(name) {
            return None;
        }
        let opts = rhai::CallFnOptions::new()
            .eval_ast(false)
            .rewind_scope(true)
            .bind_this_ptr(&mut self.state);
        self.engine
            .call_fn_with_options::<Dynamic>(opts, &mut self.scope, &self.ast, name, args)
            .ok()
    }
}

fn as_u8(d: Option<Dynamic>, default: u8) -> u8 {
    d.and_then(|d| d.as_int().ok()).map(|i| i as u8).unwrap_or(default)
}

impl BusResponder for ScriptedDevice {
    fn spi_transfer(&mut self, mosi: u8) -> u8 {
        as_u8(self.call("spi_transfer", vec![(mosi as i64).into()]), 0xFF)
    }
    fn i2c_start(&mut self) {
        self.call("i2c_start", vec![]);
    }
    fn i2c_address(&mut self, addr: u8, read: bool) -> bool {
        self.call("i2c_address", vec![(addr as i64).into(), read.into()])
            .and_then(|d| d.as_bool().ok())
            .unwrap_or(false)
    }
    fn i2c_write(&mut self, byte: u8) -> bool {
        self.call("i2c_write", vec![(byte as i64).into()])
            .and_then(|d| d.as_bool().ok())
            .unwrap_or(true)
    }
    fn i2c_read(&mut self, last: bool) -> u8 {
        as_u8(self.call("i2c_read", vec![last.into()]), 0xFF)
    }
    fn i2c_stop(&mut self) {
        self.call("i2c_stop", vec![]);
    }
    fn uart_tx(&mut self, byte: u8) {
        self.call("uart_tx", vec![(byte as i64).into()]);
    }
    fn uart_poll(&mut self) -> Option<u8> {
        match self.call("uart_poll", vec![]) {
            Some(d) if !d.is_unit() => d.as_int().ok().map(|i| i as u8),
            _ => None,
        }
    }
    fn tick(&mut self, cycles: u64) {
        self.call("tick", vec![(cycles as i64).into()]);
    }
}

/// Several devices on the buses, multiplexed: the VM holds one of these as its
/// responder. I2C is routed by address; SPI/UART go to the attached device(s).
#[derive(Default)]
pub struct DeviceBus {
    uart: Vec<ScriptedDevice>,
    spi: Vec<ScriptedDevice>,
    i2c: Vec<ScriptedDevice>,
    active_i2c: Option<usize>,
}

impl DeviceBus {
    pub fn add(&mut self, dev: ScriptedDevice) {
        match dev.bus {
            Bus::Uart => self.uart.push(dev),
            Bus::Spi => self.spi.push(dev),
            Bus::I2c => self.i2c.push(dev),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.uart.is_empty() && self.spi.is_empty() && self.i2c.is_empty()
    }
}

impl BusResponder for DeviceBus {
    fn spi_transfer(&mut self, mosi: u8) -> u8 {
        match self.spi.first_mut() {
            Some(d) => d.spi_transfer(mosi),
            None => 0xFF,
        }
    }
    fn i2c_start(&mut self) {
        self.active_i2c = None;
        for d in &mut self.i2c {
            d.i2c_start();
        }
    }
    fn i2c_address(&mut self, addr: u8, read: bool) -> bool {
        for (i, d) in self.i2c.iter_mut().enumerate() {
            if d.address == Some(addr) {
                let ack = d.i2c_address(addr, read);
                if ack {
                    self.active_i2c = Some(i);
                }
                return ack;
            }
        }
        false
    }
    fn i2c_write(&mut self, byte: u8) -> bool {
        match self.active_i2c {
            Some(i) => self.i2c[i].i2c_write(byte),
            None => false,
        }
    }
    fn i2c_read(&mut self, last: bool) -> u8 {
        match self.active_i2c {
            Some(i) => self.i2c[i].i2c_read(last),
            None => 0xFF,
        }
    }
    fn i2c_stop(&mut self) {
        if let Some(i) = self.active_i2c {
            self.i2c[i].i2c_stop();
        }
        self.active_i2c = None;
    }
    fn uart_tx(&mut self, byte: u8) {
        for d in &mut self.uart {
            d.uart_tx(byte);
        }
    }
    fn uart_poll(&mut self) -> Option<u8> {
        for d in &mut self.uart {
            if let Some(b) = d.uart_poll() {
                return Some(b);
            }
        }
        None
    }
    fn tick(&mut self, cycles: u64) {
        for d in self.uart.iter_mut().chain(self.spi.iter_mut()).chain(self.i2c.iter_mut()) {
            d.tick(cycles);
        }
    }
}

/// Built-in device scripts baked into the binary. Users can add more by
/// dropping `.rhai` files (see [`user_scripts`]).
const BUILTIN: &[(&str, &str)] = &[
    ("uart_loopback", include_str!("../../assets/devices/uart_loopback.rhai")),
    ("i2c_eeprom", include_str!("../../assets/devices/i2c_eeprom.rhai")),
    ("spi_echo", include_str!("../../assets/devices/spi_echo.rhai")),
];

/// A fresh scripting engine. Engines are cheap to share via `Arc`; one is
/// enough for every device since per-device state lives in the device.
pub fn engine() -> Arc<Engine> {
    Arc::new(Engine::new())
}

/// Load `.rhai` files from a user `devices/` directory next to the project, if
/// present, as `(id, source)` pairs.
fn user_scripts() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir("devices") {
        for e in read.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rhai") {
                if let Ok(src) = std::fs::read_to_string(&p) {
                    let id = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    out.push((id, src));
                }
            }
        }
    }
    out
}

/// The catalog of available devices (built-ins plus any user scripts), each
/// compiled once to read its descriptor. Invalid scripts are skipped.
pub fn catalog() -> Vec<DeviceSpec> {
    let eng = engine();
    let mut specs = Vec::new();
    let builtins = BUILTIN.iter().map(|(id, src)| (id.to_string(), src.to_string()));
    for (id, src) in builtins.chain(user_scripts()) {
        if let Ok(dev) = ScriptedDevice::from_src(eng.clone(), &src) {
            specs.push(DeviceSpec { id, name: dev.name, bus: dev.bus, address: dev.address, src });
        }
    }
    specs
}

/// Build a [`DeviceBus`] from the catalog ids the user attached.
pub fn build_bus(ids: &[String]) -> DeviceBus {
    let eng = engine();
    let catalog = catalog();
    let mut bus = DeviceBus::default();
    for id in ids {
        if let Some(spec) = catalog.iter().find(|s| &s.id == id) {
            if let Ok(dev) = ScriptedDevice::from_src(eng.clone(), &spec.src) {
                bus.add(dev);
            }
        }
    }
    bus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runner;

    /// An I2C EEPROM device (Rhai script) wired via DeviceBus answers a real
    /// register-level master sequence: write pointer + byte, then read it back.
    #[test]
    fn scripted_i2c_eeprom_round_trips() {
        let bus = build_bus(&["i2c_eeprom".to_string()]);
        assert!(!bus.is_empty(), "the eeprom device should load");

        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(bus));

        const TWDR: u32 = 0xBB;
        const TWCR: u32 = 0xBC;
        const START: u8 = 0xA4;
        const EN: u8 = 0x84;

        // Write 0x5A to register 0x10.
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0); // addr 0x50 + write
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x10); // pointer
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x5A); // data
        vm.write_data(TWCR, EN);

        // Point at 0x10 again, then read it back.
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x10);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA1); // addr 0x50 + read
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN); // read NACK
        assert_eq!(vm.read_data(TWDR), 0x5A, "EEPROM should return the stored byte");
    }

    /// A scripted SPI device responds on MISO.
    #[test]
    fn scripted_spi_echo_responds() {
        let bus = build_bus(&["spi_echo".to_string()]);
        let mut vm = runner::build_vm("atmega328p");
        vm.responder = Some(Box::new(bus));
        vm.write_data(0x4E, 0x20); // SPDR
        assert_eq!(vm.read_data(0x4E), 0x21);
    }
}
