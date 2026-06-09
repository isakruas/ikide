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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use ik8bvm::core::BusResponder;
use rhai::{AST, Dynamic, Engine, Map, Scope};

use crate::core::board;

/// A device's pixel output: a simple RGB framebuffer shared (via `Arc<Mutex>`)
/// between the device (on the sim thread) and the breadboard's display panel.
pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    /// 0x00RRGGBB per pixel.
    pub pixels: Vec<u32>,
    /// Bumped on every change so the UI only re-uploads the texture when needed.
    pub generation: u64,
}

/// A cloneable handle to a [`Framebuffer`], usable both as a Rhai value (a
/// device script draws through it) and by the UI (which renders it).
#[derive(Clone)]
pub struct DisplayHandle(pub Arc<Mutex<Framebuffer>>);

impl DisplayHandle {
    fn new(w: usize, h: usize) -> Self {
        DisplayHandle(Arc::new(Mutex::new(Framebuffer {
            w,
            h,
            pixels: vec![0; w * h],
            generation: 1,
        })))
    }
    fn set_px(&self, x: i64, y: i64, color: i64) {
        if let Ok(mut fb) = self.0.lock() {
            if x >= 0 && y >= 0 && (x as usize) < fb.w && (y as usize) < fb.h {
                let idx = y as usize * fb.w + x as usize;
                fb.pixels[idx] = (color as u32) & 0x00FF_FFFF;
                fb.generation = fb.generation.wrapping_add(1);
            }
        }
    }
    fn fill(&self, color: i64) {
        if let Ok(mut fb) = self.0.lock() {
            let c = (color as u32) & 0x00FF_FFFF;
            for p in fb.pixels.iter_mut() {
                *p = c;
            }
            fb.generation = fb.generation.wrapping_add(1);
        }
    }
}

/// A display surface exposed to the breadboard UI for rendering.
#[derive(Clone)]
pub struct DisplayInfo {
    pub name: String,
    pub handle: DisplayHandle,
}

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
    pub has_display: bool,
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
    /// Control pins the device watches: (script pin name, PORT data-space addr, bit).
    pins: Vec<(String, u32, u8)>,
    pin_levels: HashMap<String, u8>,
    /// Pixel output, when the device declared a `display` in its meta.
    pub display: Option<DisplayHandle>,
    pub name: String,
    pub bus: Bus,
    pub address: Option<u8>,
}

impl ScriptedDevice {
    /// Compile a device script and read its descriptor + initial state. Control
    /// pins named in `meta().pins` are resolved against `target`'s pin map.
    pub fn from_src(engine: Arc<Engine>, src: &str, target: &str) -> Result<Self, String> {
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

        // Resolve named control pins (e.g. dc: "PB1") to PORT addresses.
        let board_pins = board::pins(target);
        let mut pins = Vec::new();
        if let Some(map) = meta.get("pins").and_then(|d| d.clone().try_cast::<Map>()) {
            for (pin_name, pin_ref) in map.iter() {
                if let Ok(target_name) = pin_ref.clone().into_string() {
                    if let Some(p) = board_pins.iter().find(|p| p.name == target_name) {
                        pins.push((pin_name.to_string(), p.port_addr, p.bit));
                    }
                }
            }
        }

        // Optional pixel output.
        let display = meta.get("display").and_then(|d| d.clone().try_cast::<Map>()).map(|m| {
            let w = m.get("w").and_then(|d| d.as_int().ok()).unwrap_or(128) as usize;
            let h = m.get("h").and_then(|d| d.as_int().ok()).unwrap_or(128) as usize;
            DisplayHandle::new(w, h)
        });

        // Initial state: the script's optional init() return, else an empty map.
        let mut state = if fns.contains("init") {
            engine
                .call_fn::<Dynamic>(&mut scope, &ast, "init", ())
                .map_err(|e| format!("init(): {}", e))?
        } else {
            Dynamic::from(Map::new())
        };
        // Hand the display surface to the script as `this.fb`.
        if let Some(h) = &display {
            if let Some(mut m) = state.write_lock::<Map>() {
                m.insert("fb".into(), Dynamic::from(h.clone()));
            }
        }

        Ok(ScriptedDevice {
            engine,
            ast,
            scope,
            state,
            fns,
            pins,
            pin_levels: HashMap::new(),
            display,
            name,
            bus,
            address,
        })
    }

    /// PORT register `addr` was written: update any control pins on it and
    /// notify the script of changes via `pin_set(name, level)`.
    fn handle_pin_write(&mut self, addr: u32, value: u8) {
        let mut changes = Vec::new();
        for (pin_name, paddr, bit) in &self.pins {
            if *paddr == addr {
                let level = (value >> bit) & 1;
                if self.pin_levels.get(pin_name).copied() != Some(level) {
                    changes.push((pin_name.clone(), level));
                }
            }
        }
        for (pin_name, level) in changes {
            self.pin_levels.insert(pin_name.clone(), level);
            self.call("pin_set", vec![pin_name.into(), (level as i64).into()]);
        }
    }

    /// The PORT data-space addresses this device watches.
    fn pin_addrs(&self) -> impl Iterator<Item = u32> + '_ {
        self.pins.iter().map(|(_, a, _)| *a)
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
    fn all_mut(&mut self) -> impl Iterator<Item = &mut ScriptedDevice> {
        self.uart.iter_mut().chain(self.spi.iter_mut()).chain(self.i2c.iter_mut())
    }
    /// Every PORT data-space address any attached device watches (for the VM's
    /// `watch_pins`).
    pub fn pin_addrs(&self) -> Vec<u32> {
        self.uart
            .iter()
            .chain(self.spi.iter())
            .chain(self.i2c.iter())
            .flat_map(|d| d.pin_addrs())
            .collect()
    }
    /// The display surfaces of attached devices, for the UI to render.
    pub fn displays(&self) -> Vec<DisplayInfo> {
        self.uart
            .iter()
            .chain(self.spi.iter())
            .chain(self.i2c.iter())
            .filter_map(|d| d.display.as_ref().map(|h| DisplayInfo { name: d.name.clone(), handle: h.clone() }))
            .collect()
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
        for d in self.all_mut() {
            d.tick(cycles);
        }
    }
    fn pin_write(&mut self, addr: u32, value: u8) {
        for d in self.all_mut() {
            d.handle_pin_write(addr, value);
        }
    }
}

/// Built-in device scripts baked into the binary. Users can add more by
/// dropping `.rhai` files (see [`user_scripts`]).
const BUILTIN: &[(&str, &str)] = &[
    ("uart_loopback", include_str!("../../assets/devices/uart_loopback.rhai")),
    ("i2c_eeprom", include_str!("../../assets/devices/i2c_eeprom.rhai")),
    ("spi_echo", include_str!("../../assets/devices/spi_echo.rhai")),
    ("pcf8574", include_str!("../../assets/devices/pcf8574.rhai")),
    ("at24c256", include_str!("../../assets/devices/at24c256.rhai")),
    ("st7789", include_str!("../../assets/devices/st7789.rhai")),
];

/// A fresh scripting engine. Engines are cheap to share via `Arc`; one is
/// enough for every device since per-device state lives in the device.
pub fn engine() -> Arc<Engine> {
    let mut e = Engine::new();
    // A device handler runs inside the VM step, so cap its work to keep a buggy
    // or runaway script from wedging the simulation thread.
    e.set_max_operations(2_000_000);
    // Allow reasonably involved protocol decoders (nested if/else chains).
    e.set_max_expr_depths(128, 128);
    // Expose the pixel-output surface to display device scripts as `this.fb`.
    e.register_type_with_name::<DisplayHandle>("Display")
        .register_fn("px", |d: &mut DisplayHandle, x: i64, y: i64, color: i64| d.set_px(x, y, color))
        .register_fn("fill", |d: &mut DisplayHandle, color: i64| d.fill(color));
    Arc::new(e)
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
        // The catalog only needs the descriptor, so pin resolution is skipped.
        match ScriptedDevice::from_src(eng.clone(), &src, "") {
            Ok(dev) => specs.push(DeviceSpec {
                id,
                name: dev.name,
                bus: dev.bus,
                address: dev.address,
                has_display: dev.display.is_some(),
                src,
            }),
            Err(e) => eprintln!("[device {}] catalog build failed: {}", id, e),
        }
    }
    specs
}

/// Build a [`DeviceBus`] from the catalog ids the user attached, resolving each
/// device's control pins against the `target` MCU.
pub fn build_bus(target: &str, ids: &[String]) -> DeviceBus {
    let eng = engine();
    let catalog = catalog();
    let mut bus = DeviceBus::default();
    for id in ids {
        if let Some(spec) = catalog.iter().find(|s| &s.id == id) {
            match ScriptedDevice::from_src(eng.clone(), &spec.src, target) {
                Ok(dev) => bus.add(dev),
                Err(e) => eprintln!("[device {}] build failed: {}", id, e),
            }
        } else {
            eprintln!("[device {}] not in catalog", id);
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
        let bus = build_bus("atmega328p", &["i2c_eeprom".to_string()]);
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

    /// The AT24C256 model uses a 16-bit memory address (high byte, low byte):
    /// write a byte at 0x1234, then read it back.
    #[test]
    fn scripted_at24c256_16bit_addressing() {
        let bus = build_bus("atmega328p", &["at24c256".to_string()]);
        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(bus));

        const TWDR: u32 = 0xBB;
        const TWCR: u32 = 0xBC;
        const START: u8 = 0xA4;
        const EN: u8 = 0x84;

        // Write 0x7E to address 0x1234.
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0); // 0x50 + write
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x12); // addr high
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x34); // addr low
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x7E); // data
        vm.write_data(TWCR, EN);

        // Set the pointer to 0x1234 again, then read.
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x12);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x34);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA1); // 0x50 + read
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN); // read NACK
        assert_eq!(vm.read_data(TWDR), 0x7E, "should read back the byte at 0x1234");
    }

    /// A full display device: the VM forwards D/C pin writes and SPI bytes to
    /// the ST7789 script, which decodes RAMWR pixels into the framebuffer.
    #[test]
    fn st7789_renders_a_pixel() {
        let bus = build_bus("atmega328p", &["st7789".to_string()]);
        let display = bus.displays().first().expect("st7789 has a display").handle.clone();

        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.watch_pins = [0x25].into_iter().collect(); // PORTB (atmega328p)
        vm.responder = Some(Box::new(bus));

        const SPDR: u32 = 0x4E;
        const PORTB: u32 = 0x25;
        // DC=PB1 (bit1), CS=PB2 (bit2). Select + command mode: both low.
        vm.write_data(PORTB, 0x00);
        vm.write_data(SPDR, 0x2C); // RAMWR
        // Data mode: DC high, CS still low.
        vm.write_data(PORTB, 0x02);
        vm.write_data(SPDR, 0xF8); // RGB565 red, high byte
        vm.write_data(SPDR, 0x00); // low byte -> 0xF800

        let fb = display.0.lock().unwrap();
        assert_eq!(fb.pixels[0], 0xF8_0000, "pixel (0,0) should be red");
    }

    /// A scripted SPI device responds on MISO.
    #[test]
    fn scripted_spi_echo_responds() {
        let bus = build_bus("atmega328p", &["spi_echo".to_string()]);
        let mut vm = runner::build_vm("atmega328p");
        vm.responder = Some(Box::new(bus));
        vm.write_data(0x4E, 0x20); // SPDR
        assert_eq!(vm.read_data(0x4E), 0x21);
    }
}
