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

// Virtual devices: every breadboard part — from a bare LED to an SPI display —
// is a Rhai script. A script declares its interface in `meta()`:
//
//   fn meta() {
//       #{
//           name: "Push Button",
//           bus: "none",                       // none | uart | spi | i2c
//           pins: #{ pin: #{ mode: "drive", idle: 1 } },
//           view: [#{ kind: "button", pin: "pin", press: 0 }],
//       }
//   }
//
// `pins` are the terminals the user wires to MCU pins in the UI. Modes:
//   watch — the device observes an MCU output (script gets pin_set(name, lvl))
//   drive — the device drives an MCU input (a button pulling a pin low)
//   adc   — the terminal is an ADC channel (a potentiometer wiper)
//
// `view` is the visual interface, rendered by the breadboard automatically:
//   led | rgbled | ledbar | sevenseg | button | slider | text
// An element binds to a terminal (`pin:`/`pins:`) or to a state key (`id:`)
// the script updates — so an I2C expander can light its own LEDs.
//
// Optional handlers give a device behaviour: spi_transfer, i2c_address,
// i2c_write, i2c_read, i2c_stop, uart_tx, uart_poll, pin_set, on_view, tick.
// State persists in `this` between calls. A script can drive its `drive` pins
// by setting `this.drive = #{ pin_name: level }`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ik8bvm::core::BusResponder;
use rhai::{AST, Dynamic, Engine, Map, Scope};

use crate::core::board;

// ---------------------------------------------------------------------------
// Pixel output (displays)
// ---------------------------------------------------------------------------

/// RGB framebuffer shared between a device (sim thread) and the UI.
pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    /// 0x00RRGGBB per pixel.
    pub pixels: Vec<u32>,
    /// Bumped on change so the UI re-uploads the texture only when needed.
    pub generation: u64,
}

/// Cloneable framebuffer handle; also a Rhai type (`this.fb`) with `px`/`fill`.
#[derive(Clone)]
pub struct DisplayHandle(pub Arc<Mutex<Framebuffer>>);

impl DisplayHandle {
    fn new(w: usize, h: usize) -> Self {
        DisplayHandle(Arc::new(Mutex::new(Framebuffer { w, h, pixels: vec![0; w * h], generation: 1 })))
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

/// A display surface plus its owning device's name, for the UI.
#[derive(Clone)]
pub struct DisplayInfo {
    pub name: String,
    pub handle: DisplayHandle,
}

// ---------------------------------------------------------------------------
// Device interface model (parsed from meta())
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    None,
    Uart,
    Spi,
    I2c,
}

impl Bus {
    fn parse(s: &str) -> Option<Bus> {
        match s {
            "none" | "gpio" => Some(Bus::None),
            "uart" => Some(Bus::Uart),
            "spi" => Some(Bus::Spi),
            "i2c" | "twi" => Some(Bus::I2c),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Bus::None => "GPIO",
            Bus::Uart => "UART",
            Bus::Spi => "SPI",
            Bus::I2c => "I2C",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMode {
    /// Device observes an MCU output pin.
    Watch,
    /// Device drives an MCU input pin; `idle` is the released level.
    Drive { idle: u8 },
    /// Terminal is an ADC channel.
    Adc,
}

#[derive(Clone, Debug)]
pub struct PinDef {
    pub name: String,
    pub mode: PinMode,
    /// Suggested MCU pin (e.g. "PB1"); the user can rewire in the UI.
    pub default_pin: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Led,
    RgbLed,
    LedBar,
    SevenSeg,
    Button,
    Slider,
    Text,
}

/// One element of a device's visual interface.
#[derive(Clone, Debug)]
pub struct ViewDef {
    pub kind: ViewKind,
    /// State key this element binds to (script-updated), if any.
    pub id: Option<String>,
    /// Terminals this element binds to (rendered straight from pin state).
    pub pins: Vec<String>,
    pub color: u32,
    /// Button: level driven while pressed.
    pub press: u8,
    /// Slider range.
    pub min: f64,
    pub max: f64,
    /// LedBar width when id-bound.
    pub bits: usize,
    pub label: Option<String>,
}

/// A catalog entry: everything the UI needs before instantiation.
#[derive(Clone)]
pub struct DeviceSpec {
    pub id: String,
    pub name: String,
    pub bus: Bus,
    pub address: Option<u8>,
    pub pins: Vec<PinDef>,
    pub view: Vec<ViewDef>,
    pub has_display: bool,
    src: String,
}

/// A value exported from a device's state for the UI (via view `id`s).
#[derive(Clone, Debug)]
pub enum ViewVal {
    Num(f64),
    Str(String),
}

/// How one device instance is wired: terminal name -> MCU pin name / channel.
#[derive(Clone, Debug, Default)]
pub struct InstanceWiring {
    pub spec_id: String,
    pub pins: HashMap<String, String>,
    pub adc: HashMap<String, u8>,
}

// ---------------------------------------------------------------------------
// meta() parsing
// ---------------------------------------------------------------------------

fn map_of(d: &Dynamic) -> Option<Map> {
    d.clone().try_cast::<Map>()
}

fn str_of(d: &Dynamic) -> Option<String> {
    d.clone().into_string().ok()
}

fn int_of(d: &Dynamic) -> Option<i64> {
    d.as_int().ok()
}

struct Meta {
    name: String,
    bus: Bus,
    address: Option<u8>,
    pins: Vec<PinDef>,
    view: Vec<ViewDef>,
    display: Option<(usize, usize)>,
}

fn parse_meta(engine: &Engine, ast: &AST) -> Result<Meta, String> {
    let mut scope = Scope::new();
    let meta: Map = engine.call_fn(&mut scope, ast, "meta", ()).map_err(|e| format!("meta(): {}", e))?;

    let name = meta.get("name").and_then(str_of).unwrap_or_else(|| "device".into());
    let bus = match meta.get("bus") {
        Some(d) => str_of(d).and_then(|s| Bus::parse(&s)).ok_or("bus must be none|uart|spi|i2c")?,
        None => Bus::None,
    };
    let address = meta.get("address").and_then(int_of).map(|i| i as u8);

    let mut pins = Vec::new();
    if let Some(map) = meta.get("pins").and_then(map_of) {
        for (pin_name, def) in map.iter() {
            // Shorthand `dc: "PB1"` = a watch pin with a default wiring.
            if let Some(default) = str_of(def) {
                pins.push(PinDef { name: pin_name.to_string(), mode: PinMode::Watch, default_pin: Some(default) });
                continue;
            }
            let m = map_of(def).unwrap_or_default();
            let mode = match m.get("mode").and_then(str_of).as_deref() {
                Some("drive") => PinMode::Drive { idle: m.get("idle").and_then(int_of).unwrap_or(1) as u8 },
                Some("adc") => PinMode::Adc,
                _ => PinMode::Watch,
            };
            let default_pin = m.get("default").and_then(str_of);
            pins.push(PinDef { name: pin_name.to_string(), mode, default_pin });
        }
    }

    let mut view = Vec::new();
    if let Some(arr) = meta.get("view").and_then(|d| d.clone().try_cast::<rhai::Array>()) {
        for el in arr.iter() {
            let m = match map_of(el) {
                Some(m) => m,
                None => continue,
            };
            let kind = match m.get("kind").and_then(str_of).as_deref() {
                Some("led") => ViewKind::Led,
                Some("rgbled" | "rgb_led") => ViewKind::RgbLed,
                Some("ledbar" | "led_bar") => ViewKind::LedBar,
                Some("sevenseg" | "seven_seg" | "7seg") => ViewKind::SevenSeg,
                Some("button") => ViewKind::Button,
                Some("slider") => ViewKind::Slider,
                Some("text") => ViewKind::Text,
                _ => continue,
            };
            let mut el_pins = Vec::new();
            if let Some(p) = m.get("pin").and_then(str_of) {
                el_pins.push(p);
            }
            if let Some(arr) = m.get("pins").and_then(|d| d.clone().try_cast::<rhai::Array>()) {
                el_pins.extend(arr.iter().filter_map(str_of));
            }
            view.push(ViewDef {
                kind,
                id: m.get("id").and_then(str_of),
                pins: el_pins,
                color: m.get("color").and_then(int_of).unwrap_or(0xFF5252) as u32,
                press: m.get("press").and_then(int_of).unwrap_or(0) as u8,
                min: m.get("min").and_then(int_of).unwrap_or(0) as f64,
                max: m.get("max").and_then(int_of).unwrap_or(1023) as f64,
                bits: m.get("bits").and_then(int_of).unwrap_or(8) as usize,
                label: m.get("label").and_then(str_of),
            });
        }
    }

    let display = meta.get("display").and_then(map_of).map(|m| {
        (
            m.get("w").and_then(int_of).unwrap_or(128) as usize,
            m.get("h").and_then(int_of).unwrap_or(128) as usize,
        )
    });

    Ok(Meta { name, bus, address, pins, view, display })
}

// ---------------------------------------------------------------------------
// A live device instance
// ---------------------------------------------------------------------------

pub struct ScriptedDevice {
    engine: Arc<Engine>,
    ast: AST,
    scope: Scope<'static>,
    state: Dynamic,
    fns: std::collections::HashSet<String>,
    /// Watch terminals resolved to (name, PORT addr, bit).
    watch: Vec<(String, u32, u8)>,
    /// Drive terminals resolved to (name, PIN addr, bit, idle level).
    drive: Vec<(String, u32, u8, u8)>,
    pin_levels: HashMap<String, u8>,
    /// State keys exported to the UI (the view element ids).
    view_ids: Vec<String>,
    pub display: Option<DisplayHandle>,
    pub name: String,
    pub bus: Bus,
    pub address: Option<u8>,
}

impl ScriptedDevice {
    /// Compile a script and wire its terminals for `target`. `wiring` overrides
    /// the meta defaults (terminal name -> MCU pin name).
    pub fn from_src(
        engine: Arc<Engine>,
        src: &str,
        target: &str,
        wiring: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let ast = engine.compile(src).map_err(|e| e.to_string())?;
        let fns: std::collections::HashSet<String> = ast.iter_functions().map(|f| f.name.to_string()).collect();
        let meta = parse_meta(&engine, &ast)?;

        let board_pins = board::pins(target);
        let mut watch = Vec::new();
        let mut drive = Vec::new();
        for def in &meta.pins {
            let assigned = wiring.get(&def.name).cloned().or_else(|| def.default_pin.clone());
            let pin = assigned.and_then(|n| board_pins.iter().find(|p| p.name == n));
            if let Some(p) = pin {
                match def.mode {
                    PinMode::Watch => watch.push((def.name.clone(), p.port_addr, p.bit)),
                    PinMode::Drive { idle } => drive.push((def.name.clone(), p.pin_addr, p.bit, idle)),
                    PinMode::Adc => {}
                }
            }
        }

        let display = meta.display.map(|(w, h)| DisplayHandle::new(w, h));
        let view_ids = meta.view.iter().filter_map(|v| v.id.clone()).collect();

        let mut scope = Scope::new();
        let mut state = if fns.contains("init") {
            engine
                .call_fn::<Dynamic>(&mut scope, &ast, "init", ())
                .map_err(|e| format!("init(): {}", e))?
        } else {
            Dynamic::from(Map::new())
        };
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
            watch,
            drive,
            pin_levels: HashMap::new(),
            view_ids,
            display,
            name: meta.name,
            bus: meta.bus,
            address: meta.address,
        })
    }

    /// Call a script handler with `this` bound to the device state. Missing
    /// handlers are no-ops; runtime errors abort just that call.
    fn call(&mut self, name: &str, args: Vec<Dynamic>) -> Option<Dynamic> {
        if !self.fns.contains(name) {
            return None;
        }
        let opts = rhai::CallFnOptions::new()
            .eval_ast(false)
            .rewind_scope(true)
            .bind_this_ptr(&mut self.state);
        match self.engine.call_fn_with_options::<Dynamic>(opts, &mut self.scope, &self.ast, name, args) {
            Ok(v) => Some(v),
            Err(e) => {
                // A script error aborts only this call; log it so device
                // authors can see what went wrong.
                eprintln!("[device {}] {}(): {}", self.name, name, e);
                None
            }
        }
    }

    /// A watched PORT register changed: notify the script of any edges.
    fn handle_pin_write(&mut self, addr: u32, value: u8) {
        let mut changes = Vec::new();
        for (name, paddr, bit) in &self.watch {
            if *paddr == addr {
                let level = (value >> bit) & 1;
                if self.pin_levels.get(name).copied() != Some(level) {
                    changes.push((name.clone(), level));
                }
            }
        }
        for (name, level) in changes {
            self.pin_levels.insert(name.clone(), level);
            self.call("pin_set", vec![name.into(), (level as i64).into()]);
        }
    }

    /// A UI element changed: record it in state and notify the script.
    fn ui_event(&mut self, id: &str, value: f64) {
        if let Some(mut m) = self.state.write_lock::<Map>() {
            m.insert(id.into(), (value as i64).into());
        }
        self.call("on_view", vec![id.into(), value.into()]);
    }

    /// Export the view-bound state keys for the UI.
    fn view_values(&self, out: &mut Vec<(String, ViewVal)>) {
        let m = match self.state.clone().try_cast::<Map>() {
            Some(m) => m,
            None => return,
        };
        for id in &self.view_ids {
            if let Some(v) = m.get(id.as_str()) {
                if let Ok(i) = v.as_int() {
                    out.push((id.clone(), ViewVal::Num(i as f64)));
                } else if let Ok(f) = v.as_float() {
                    out.push((id.clone(), ViewVal::Num(f)));
                } else if let Some(s) = str_of(v) {
                    out.push((id.clone(), ViewVal::Str(s)));
                }
            }
        }
    }

    /// Take pin levels the script asked to drive (`this.drive` map), resolved
    /// to (PIN addr, bit, level).
    fn take_drives(&mut self, out: &mut Vec<(u32, u8, bool)>) {
        let req: Option<Map> = self
            .state
            .write_lock::<Map>()
            .and_then(|mut m| m.remove("drive"))
            .and_then(|d| d.try_cast::<Map>());
        let req = match req {
            Some(r) => r,
            None => return,
        };
        for (name, level) in req.iter() {
            if let Some((_, addr, bit, _)) = self.drive.iter().find(|(n, ..)| n == name.as_str()) {
                if let Some(l) = int_of(level) {
                    out.push((*addr, *bit, l != 0));
                }
            }
        }
    }

    /// Idle levels of the drive terminals, asserted when the run starts.
    fn idle_drives(&self, out: &mut Vec<(u32, u8, bool)>) {
        for (_, addr, bit, idle) in &self.drive {
            out.push((*addr, *bit, *idle != 0));
        }
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
    fn pin_write(&mut self, addr: u32, value: u8) {
        self.handle_pin_write(addr, value);
    }
    fn tick(&mut self, cycles: u64) {
        self.call("tick", vec![(cycles as i64).into()]);
    }
}

// ---------------------------------------------------------------------------
// DeviceBus: all instances, multiplexed onto the buses
// ---------------------------------------------------------------------------

/// Every attached device, in instance order. Serial traffic is routed per bus
/// (I2C by address); pin writes and ticks fan out to all.
#[derive(Default)]
pub struct DeviceBus {
    devs: Vec<ScriptedDevice>,
    active_i2c: Option<usize>,
}

impl DeviceBus {
    pub fn add(&mut self, dev: ScriptedDevice) {
        self.devs.push(dev);
    }
    pub fn is_empty(&self) -> bool {
        self.devs.is_empty()
    }
    /// PORT addresses any device watches (for the VM's `watch_pins`).
    pub fn pin_addrs(&self) -> Vec<u32> {
        self.devs.iter().flat_map(|d| d.watch.iter().map(|(_, a, _)| *a)).collect()
    }
    pub fn displays(&self) -> Vec<DisplayInfo> {
        self.devs
            .iter()
            .filter_map(|d| d.display.as_ref().map(|h| DisplayInfo { name: d.name.clone(), handle: h.clone() }))
            .collect()
    }
    /// Forward a UI element change to instance `dev`.
    pub fn ui_event(&mut self, dev: usize, id: &str, value: f64) {
        if let Some(d) = self.devs.get_mut(dev) {
            d.ui_event(id, value);
        }
    }
    /// Snapshot every device's view-bound state keys: (instance, key, value).
    pub fn view_values(&self) -> Vec<(usize, String, ViewVal)> {
        let mut out = Vec::new();
        for (i, d) in self.devs.iter().enumerate() {
            let mut vals = Vec::new();
            d.view_values(&mut vals);
            out.extend(vals.into_iter().map(|(k, v)| (i, k, v)));
        }
        out
    }
    /// Pin levels requested by scripts since the last call.
    pub fn take_drives(&mut self) -> Vec<(u32, u8, bool)> {
        let mut out = Vec::new();
        for d in &mut self.devs {
            d.take_drives(&mut out);
        }
        out
    }
    /// Idle levels of every drive terminal (asserted at run start).
    pub fn idle_drives(&self) -> Vec<(u32, u8, bool)> {
        let mut out = Vec::new();
        for d in &self.devs {
            d.idle_drives(&mut out);
        }
        out
    }
}

impl BusResponder for DeviceBus {
    fn spi_transfer(&mut self, mosi: u8) -> u8 {
        match self.devs.iter_mut().find(|d| d.bus == Bus::Spi) {
            Some(d) => d.spi_transfer(mosi),
            None => 0xFF,
        }
    }
    fn i2c_start(&mut self) {
        self.active_i2c = None;
        for d in self.devs.iter_mut().filter(|d| d.bus == Bus::I2c) {
            d.i2c_start();
        }
    }
    fn i2c_address(&mut self, addr: u8, read: bool) -> bool {
        for (i, d) in self.devs.iter_mut().enumerate() {
            if d.bus == Bus::I2c && d.address == Some(addr) {
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
            Some(i) => self.devs[i].i2c_write(byte),
            None => false,
        }
    }
    fn i2c_read(&mut self, last: bool) -> u8 {
        match self.active_i2c {
            Some(i) => self.devs[i].i2c_read(last),
            None => 0xFF,
        }
    }
    fn i2c_stop(&mut self) {
        if let Some(i) = self.active_i2c {
            self.devs[i].i2c_stop();
        }
        self.active_i2c = None;
    }
    fn uart_tx(&mut self, byte: u8) {
        for d in self.devs.iter_mut().filter(|d| d.bus == Bus::Uart) {
            d.uart_tx(byte);
        }
    }
    fn uart_poll(&mut self) -> Option<u8> {
        for d in self.devs.iter_mut().filter(|d| d.bus == Bus::Uart) {
            if let Some(b) = d.uart_poll() {
                return Some(b);
            }
        }
        None
    }
    fn pin_write(&mut self, addr: u32, value: u8) {
        for d in &mut self.devs {
            d.handle_pin_write(addr, value);
        }
    }
    fn tick(&mut self, cycles: u64) {
        for d in &mut self.devs {
            d.tick(cycles);
        }
    }
}

/// The engine keeps the bus shared so it can pump UI events and snapshots
/// while the VM owns a responder view of the same devices. Both live on the
/// simulation thread, so the lock is never contended.
#[derive(Clone)]
pub struct SharedBus(pub Arc<Mutex<DeviceBus>>);

impl BusResponder for SharedBus {
    fn spi_transfer(&mut self, mosi: u8) -> u8 {
        self.0.lock().map(|mut b| b.spi_transfer(mosi)).unwrap_or(0xFF)
    }
    fn i2c_start(&mut self) {
        if let Ok(mut b) = self.0.lock() {
            b.i2c_start();
        }
    }
    fn i2c_address(&mut self, addr: u8, read: bool) -> bool {
        self.0.lock().map(|mut b| b.i2c_address(addr, read)).unwrap_or(false)
    }
    fn i2c_write(&mut self, byte: u8) -> bool {
        self.0.lock().map(|mut b| b.i2c_write(byte)).unwrap_or(false)
    }
    fn i2c_read(&mut self, last: bool) -> u8 {
        self.0.lock().map(|mut b| b.i2c_read(last)).unwrap_or(0xFF)
    }
    fn i2c_stop(&mut self) {
        if let Ok(mut b) = self.0.lock() {
            b.i2c_stop();
        }
    }
    fn uart_tx(&mut self, byte: u8) {
        if let Ok(mut b) = self.0.lock() {
            b.uart_tx(byte);
        }
    }
    fn uart_poll(&mut self) -> Option<u8> {
        self.0.lock().ok().and_then(|mut b| b.uart_poll())
    }
    fn pin_write(&mut self, addr: u32, value: u8) {
        if let Ok(mut b) = self.0.lock() {
            b.pin_write(addr, value);
        }
    }
    fn tick(&mut self, cycles: u64) {
        if let Ok(mut b) = self.0.lock() {
            b.tick(cycles);
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// Built-in device scripts. Users add more by dropping `.rhai` files in a
/// `devices/` directory next to their project — no recompile.
const BUILTIN: &[(&str, &str)] = &[
    ("led", include_str!("../../assets/devices/led.rhai")),
    ("rgb_led", include_str!("../../assets/devices/rgb_led.rhai")),
    ("push_button", include_str!("../../assets/devices/push_button.rhai")),
    ("seven_segment", include_str!("../../assets/devices/seven_segment.rhai")),
    ("led_bar", include_str!("../../assets/devices/led_bar.rhai")),
    ("potentiometer", include_str!("../../assets/devices/potentiometer.rhai")),
    ("uart_loopback", include_str!("../../assets/devices/uart_loopback.rhai")),
    ("i2c_eeprom", include_str!("../../assets/devices/i2c_eeprom.rhai")),
    ("spi_echo", include_str!("../../assets/devices/spi_echo.rhai")),
    ("pcf8574", include_str!("../../assets/devices/pcf8574.rhai")),
    ("at24c256", include_str!("../../assets/devices/at24c256.rhai")),
    ("st7789", include_str!("../../assets/devices/st7789.rhai")),
    ("ssd1306", include_str!("../../assets/devices/ssd1306.rhai")),
    ("hd44780_i2c", include_str!("../../assets/devices/hd44780_i2c.rhai")),
    ("sn74hc595", include_str!("../../assets/devices/sn74hc595.rhai")),
    ("max7219", include_str!("../../assets/devices/max7219.rhai")),
    ("lm75", include_str!("../../assets/devices/lm75.rhai")),
    ("mpu6050", include_str!("../../assets/devices/mpu6050.rhai")),
    ("joystick", include_str!("../../assets/devices/joystick.rhai")),
];

/// Source of a built-in (shipped) device by name, e.g. `"at24c256"` — the same
/// catalog the breadboard offers. Returns `None` for an unknown name.
pub fn builtin_device_src(name: &str) -> Option<&'static str> {
    BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// One scripting engine for all devices; per-device state lives in the device.
pub fn engine() -> Arc<Engine> {
    let mut e = Engine::new();
    // Handlers run inside VM steps: cap work so a runaway script can't wedge
    // the simulation, and allow deep protocol-decoder if/else chains.
    e.set_max_operations(2_000_000);
    e.set_max_expr_depths(128, 128);
    e.register_type_with_name::<DisplayHandle>("Display")
        .register_fn("px", |d: &mut DisplayHandle, x: i64, y: i64, c: i64| d.set_px(x, y, c))
        .register_fn("fill", |d: &mut DisplayHandle, c: i64| d.fill(c));
    // chr(code): one-character string from an ASCII code, for text views.
    e.register_fn("chr", |c: i64| {
        char::from_u32((c as u32).clamp(0x20, 0x10FFFF)).unwrap_or(' ').to_string()
    });
    Arc::new(e)
}

fn user_scripts() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir("devices") {
        for e in read.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rhai") {
                if let Ok(src) = std::fs::read_to_string(&p) {
                    out.push((p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), src));
                }
            }
        }
    }
    out
}

/// All available devices (built-ins + user scripts). Broken scripts are
/// reported on stderr and skipped.
pub fn catalog() -> Vec<DeviceSpec> {
    let eng = engine();
    let mut specs = Vec::new();
    let builtins = BUILTIN.iter().map(|(id, src)| (id.to_string(), src.to_string()));
    for (id, src) in builtins.chain(user_scripts()) {
        let ast = match eng.compile(&src) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[device {}] compile error: {}", id, e);
                continue;
            }
        };
        match parse_meta(&eng, &ast) {
            Ok(meta) => specs.push(DeviceSpec {
                id,
                name: meta.name,
                bus: meta.bus,
                address: meta.address,
                pins: meta.pins,
                view: meta.view,
                has_display: meta.display.is_some(),
                src,
            }),
            Err(e) => eprintln!("[device {}] {}", id, e),
        }
    }
    specs
}

/// Instantiate the wired devices for a run.
pub fn build_bus(target: &str, instances: &[InstanceWiring]) -> DeviceBus {
    let eng = engine();
    let catalog = catalog();
    let mut bus = DeviceBus::default();
    for inst in instances {
        let spec = match catalog.iter().find(|s| s.id == inst.spec_id) {
            Some(s) => s,
            None => continue,
        };
        match ScriptedDevice::from_src(eng.clone(), &spec.src, target, &inst.pins) {
            Ok(dev) => bus.add(dev),
            Err(e) => eprintln!("[device {}] {}", inst.spec_id, e),
        }
    }
    bus
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runner;

    fn wired(spec_id: &str) -> InstanceWiring {
        InstanceWiring { spec_id: spec_id.into(), ..Default::default() }
    }

    fn vm_with(bus: DeviceBus) -> ik8bvm::core::AvrVm {
        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(bus));
        vm
    }

    // atmega328p TWI registers and TWCR command values.
    const TWDR: u32 = 0xBB;
    const TWCR: u32 = 0xBC;
    const START: u8 = 0xA4;
    const EN: u8 = 0x84;

    /// Every shipped example program compiles for its declared target.
    #[test]
    fn all_examples_compile() {
        let mut checked = 0;
        let original_dir = std::env::current_dir().expect("get current dir");
        for entry in std::fs::read_dir("assets/examples").expect("examples dir") {
            let path = entry.expect("entry").path();
            let main = path.join("main.ik");
            if !main.is_file() {
                continue;
            }
            std::env::set_current_dir(&path).expect("set current dir");
            let src = std::fs::read_to_string("main.ik").expect("read main.ik");
            crate::core::analysis::sync_std_imports(&src);
            let res = crate::core::analysis::compile(&src);
            std::env::set_current_dir(&original_dir).expect("restore current dir");
            res.unwrap_or_else(|e| panic!("{:?} failed to compile: {}", main, e.message));
            checked += 1;
        }
        assert!(checked >= 24, "expected all examples checked, got {}", checked);
    }

    /// The font-on-display example really renders: run the shipped program in
    /// the VM with the ST7789 attached and check the framebuffer — a glyph
    /// pixel of the "I" turns white and the cleared background stays dark.
    /// Tens of millions of simulated instructions: ~2 s in release, ~40 s in
    /// debug, so the debug suite skips it.
    #[test]
    #[cfg_attr(debug_assertions, ignore = "slow full render; run with --release -- --ignored")]
    fn st7789_font_example_renders_text() {
        let src = std::fs::read_to_string("assets/examples/breadboard_st7789_font/main.ik").unwrap();
        crate::core::analysis::sync_std_imports(&src);
        let art = crate::core::analysis::compile(&src).expect("example compiles");

        let hex = std::env::temp_dir().join("ikide_font_example.hex");
        std::fs::write(&hex, &art.hex).unwrap();

        let bus = build_bus("atmega328p", &[wired("st7789")]);
        let display = bus.displays().first().expect("display").handle.clone();

        let mut vm = runner::build_vm(&art.device);
        ik8bvm::hw::load_hex(&mut vm, &hex.to_string_lossy()).expect("HEX loads");
        vm.watch_pins = bus.pin_addrs().into_iter().collect();
        vm.responder = Some(Box::new(bus));

        // 'I' = glyph column 2 is a full bar: pixel (68, 110) must turn white.
        let target = 110 * 240 + 68;
        let mut steps: u64 = 0;
        while vm.running && steps < 400_000_000 {
            for _ in 0..1_000_000 {
                vm.step();
            }
            steps += 1_000_000;
            if display.0.lock().unwrap().pixels[target] == 0xFF_FFFF {
                break;
            }
        }

        let fb = display.0.lock().unwrap();
        assert_eq!(fb.pixels[target], 0xFF_FFFF, "glyph bar pixel should be white");
        assert_eq!(fb.pixels[0], 0x10_1010, "background stays dark (0x1082 in RGB565)");
    }

    /// Pure-view component scripts parse with pins and view elements.
    #[test]
    fn component_scripts_parse() {
        let specs = catalog();
        let led = specs.iter().find(|s| s.id == "led").expect("led in catalog");
        assert_eq!(led.bus, Bus::None);
        assert_eq!(led.pins.len(), 1);
        assert_eq!(led.view[0].kind, ViewKind::Led);

        let btn = specs.iter().find(|s| s.id == "push_button").unwrap();
        assert!(matches!(btn.pins[0].mode, PinMode::Drive { idle: 1 }));

        let pot = specs.iter().find(|s| s.id == "potentiometer").unwrap();
        assert!(matches!(pot.pins[0].mode, PinMode::Adc));

        let seg = specs.iter().find(|s| s.id == "seven_segment").unwrap();
        assert_eq!(seg.view[0].pins.len(), 8);

        // Joystick: six active-low drive lines, six buttons, default wiring.
        let joy = specs.iter().find(|s| s.id == "joystick").unwrap();
        assert_eq!(joy.pins.len(), 6);
        assert!(joy.pins.iter().all(|p| matches!(p.mode, PinMode::Drive { idle: 1 })));
        assert!(joy.pins.iter().all(|p| p.default_pin.is_some()));
        assert_eq!(joy.view.iter().filter(|v| v.kind == ViewKind::Button).count(), 6);
    }

    /// The PCF8574's latched port is exported to the UI via its view id —
    /// exercised through the production SharedBus path.
    #[test]
    fn pcf8574_view_state_tracks_i2c_writes() {
        let bus = Arc::new(Mutex::new(build_bus("atmega328p", &[wired("pcf8574")])));
        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.responder = Some(Box::new(SharedBus(bus.clone())));

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0x40); // 0x20 << 1 | write
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0xA5);
        vm.write_data(TWCR, EN);

        let vals = bus.lock().unwrap().view_values();
        assert!(
            vals.iter().any(|(i, k, v)| *i == 0 && k == "port" && matches!(v, ViewVal::Num(n) if *n == 165.0)),
            "port view value should be 0xA5, got {:?}",
            vals
        );
    }

    /// A script can drive its wired MCU pin (this.drive map -> PINx write).
    #[test]
    fn script_drive_reaches_pin_register() {
        let src = r#"
fn meta() {
    #{ name: "Driver", pins: #{ out: #{ mode: "drive", idle: 1 } },
       view: [#{ kind: "button", id: "b", pin: "out" }] }
}
fn on_view(id, value) {
    this.drive = #{ out: if value > 0.0 { 0 } else { 1 } };
}
"#;
        let eng = engine();
        let mut wiring = HashMap::new();
        wiring.insert("out".to_string(), "PD2".to_string());
        let dev = ScriptedDevice::from_src(eng, src, "atmega328p", &wiring).expect("compiles");
        let mut bus = DeviceBus::default();
        bus.add(dev);

        // Idle levels: PD2 (PIND=0x29, bit 2) high.
        let idles = bus.idle_drives();
        assert_eq!(idles, vec![(0x29, 2, true)]);

        // Press: the script asks to drive the pin low.
        bus.ui_event(0, "b", 1.0);
        let drives = bus.take_drives();
        assert_eq!(drives, vec![(0x29, 2, false)]);
    }

    /// I2C EEPROM round-trip through the VM's TWI master (pointer + data).
    #[test]
    fn scripted_i2c_eeprom_round_trips() {
        let bus = build_bus("atmega328p", &[wired("i2c_eeprom")]);
        assert!(!bus.is_empty());
        let mut vm = vm_with(bus);

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x10);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x5A);
        vm.write_data(TWCR, EN);

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x10);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA1);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN);
        assert_eq!(vm.read_data(TWDR), 0x5A);
    }

    /// AT24C256 uses a 16-bit memory address (high then low byte).
    #[test]
    fn scripted_at24c256_16bit_addressing() {
        let bus = build_bus("atmega328p", &[wired("at24c256")]);
        let mut vm = vm_with(bus);

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x12);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x34);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x7E);
        vm.write_data(TWCR, EN);

        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA0);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x12);
        vm.write_data(TWCR, EN);
        vm.write_data(TWDR, 0x34);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, 0xA1);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN);
        assert_eq!(vm.read_data(TWDR), 0x7E);
    }

    /// The ST7789 decodes RAMWR pixels into its framebuffer, gated by the D/C
    /// pin which the VM forwards on watched PORT writes.
    #[test]
    fn st7789_renders_a_pixel() {
        let bus = build_bus("atmega328p", &[wired("st7789")]);
        let display = bus.displays().first().expect("st7789 has a display").handle.clone();

        let mut vm = runner::build_vm("atmega328p");
        vm.capture_io = true;
        vm.watch_pins = bus.pin_addrs().into_iter().collect();
        vm.responder = Some(Box::new(bus));

        const SPDR: u32 = 0x4E;
        const PORTB: u32 = 0x25;
        vm.write_data(PORTB, 0x00); // DC low (command), CS low (selected)
        vm.write_data(SPDR, 0x2C); // RAMWR
        vm.write_data(PORTB, 0x02); // DC high (data)
        vm.write_data(SPDR, 0xF8); // RGB565 red
        vm.write_data(SPDR, 0x00);

        let fb = display.0.lock().unwrap();
        assert_eq!(fb.pixels[0], 0xFF_0000, "RGB565 0xF800 expands to full red");
    }

    /// A scripted SPI device answers on MISO.
    #[test]
    fn scripted_spi_echo_responds() {
        let bus = build_bus("atmega328p", &[wired("spi_echo")]);
        let mut vm = vm_with(bus);
        vm.write_data(0x4E, 0x20);
        assert_eq!(vm.read_data(0x4E), 0x21);
    }

    /// I2C helper: one full master write transaction (addr + payload bytes).
    fn twi_write(vm: &mut ik8bvm::core::AvrVm, addr7: u8, bytes: &[u8]) {
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, addr7 << 1);
        vm.write_data(TWCR, EN);
        for &b in bytes {
            vm.write_data(TWDR, b);
            vm.write_data(TWCR, EN);
        }
        vm.write_data(TWCR, 0x94); // STOP
    }

    /// SSD1306: init window + one data byte lights pixel (0,0).
    #[test]
    fn ssd1306_draws_pixels() {
        let bus = build_bus("atmega328p", &[wired("ssd1306")]);
        let display = bus.displays().first().expect("has display").handle.clone();
        let mut vm = vm_with(bus);

        twi_write(&mut vm, 0x3C, &[0x00, 0x20, 0x00, 0x21, 0, 127, 0x22, 0, 7]);
        twi_write(&mut vm, 0x3C, &[0x40, 0x01]); // data: bit 0 -> pixel (0,0)

        let fb = display.0.lock().unwrap();
        assert_ne!(fb.pixels[0], 0, "pixel (0,0) should be lit");
        assert_eq!(fb.pixels[1], 0, "pixel (1,0) stays dark");
    }

    /// HD44780 backpack: init to 4-bit mode, print 'H', read the text view.
    #[test]
    fn hd44780_renders_text() {
        let bus = Arc::new(Mutex::new(build_bus("atmega328p", &[wired("hd44780_i2c")])));
        let mut vm = runner::build_vm("atmega328p");
        vm.responder = Some(Box::new(SharedBus(bus.clone())));

        // Send one nibble with an EN pulse (RS in bit 0, nibble in bits 4..7).
        let mut nib = |vm: &mut ik8bvm::core::AvrVm, n: u8, rs: u8| {
            let base = (n << 4) | rs;
            twi_write(vm, 0x27, &[base | 0x04, base]); // EN high, EN low
        };
        nib(&mut vm, 0x2, 0); // enter 4-bit mode
        nib(&mut vm, 0x4, 1); // 'H' = 0x48: high nibble
        nib(&mut vm, 0x8, 1); //             low nibble

        let vals = bus.lock().unwrap().view_values();
        let screen = vals.iter().find_map(|(_, k, v)| match (k.as_str(), v) {
            ("screen", ViewVal::Str(s)) => Some(s.clone()),
            _ => None,
        });
        assert!(screen.unwrap_or_default().starts_with('H'), "LCD should show H");
    }

    /// 74HC595: SPI byte latched to the outputs on the latch rising edge.
    #[test]
    fn sn74hc595_latches_on_rising_edge() {
        let bus = Arc::new(Mutex::new(build_bus("atmega328p", &[wired("sn74hc595")])));
        let mut vm = runner::build_vm("atmega328p");
        vm.watch_pins = bus.lock().unwrap().pin_addrs().into_iter().collect();
        vm.responder = Some(Box::new(SharedBus(bus.clone())));

        const PORTB: u32 = 0x25;
        vm.write_data(PORTB, 0x00); // latch (PB2) low
        vm.write_data(0x4E, 0xA5); // shift a byte in
        vm.write_data(PORTB, 0x04); // latch rising edge

        let vals = bus.lock().unwrap().view_values();
        assert!(
            vals.iter().any(|(_, k, v)| k == "q" && matches!(v, ViewVal::Num(n) if *n == 165.0)),
            "outputs should latch 0xA5, got {:?}",
            vals
        );
    }

    /// MAX7219: powers up in shutdown (blank); after waking via register
    /// 0x0C, a (row, data) frame latched on LOAD lights the row's pixels.
    #[test]
    fn max7219_lights_a_row() {
        let bus = build_bus("atmega328p", &[wired("max7219")]);
        let display = bus.displays().first().expect("has display").handle.clone();
        let mut vm = runner::build_vm("atmega328p");
        vm.watch_pins = bus.pin_addrs().into_iter().collect();
        vm.responder = Some(Box::new(bus));

        const PORTB: u32 = 0x25;
        let mut frame = |vm: &mut ik8bvm::core::AvrVm, reg: u8, data: u8| {
            vm.write_data(PORTB, 0x00); // LOAD low
            vm.write_data(0x4E, reg);
            vm.write_data(0x4E, data);
            vm.write_data(PORTB, 0x04); // LOAD rising edge latches
        };

        frame(&mut vm, 0x01, 0x81); // row 0 while still in shutdown
        assert_eq!(display.0.lock().unwrap().pixels[0], 0, "blank in shutdown");

        frame(&mut vm, 0x0C, 0x01); // leave shutdown
        let fb = display.0.lock().unwrap();
        assert_ne!(fb.pixels[0], 0, "col 0 lit");
        assert_ne!(fb.pixels[7], 0, "col 7 lit");
        assert_eq!(fb.pixels[1], 0, "col 1 dark");
    }

    /// LM75: the slider temperature is read back over I2C.
    #[test]
    fn lm75_reads_set_temperature() {
        let bus = Arc::new(Mutex::new(build_bus("atmega328p", &[wired("lm75")])));
        let mut vm = runner::build_vm("atmega328p");
        vm.responder = Some(Box::new(SharedBus(bus.clone())));

        bus.lock().unwrap().ui_event(0, "temp_c", 31.0);

        twi_write(&mut vm, 0x48, &[0x00]); // point at the temperature register
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, (0x48 << 1) | 1);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, 0xC4); // read with ACK (TWEA): MSB
        let msb = vm.read_data(TWDR);
        vm.write_data(TWCR, EN); // read with NACK: LSB
        let _lsb = vm.read_data(TWDR);
        assert_eq!(msb, 31, "MSB should be the set temperature");
    }

    /// MPU6050: WHO_AM_I answers 0x68 and the accel block reads 1 g on Z.
    #[test]
    fn mpu6050_whoami_and_accel() {
        let bus = build_bus("atmega328p", &[wired("mpu6050")]);
        let mut vm = vm_with(bus);

        twi_write(&mut vm, 0x68, &[0x75]); // WHO_AM_I
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, (0x68 << 1) | 1);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN); // read NACK
        assert_eq!(vm.read_data(TWDR), 0x68);

        twi_write(&mut vm, 0x68, &[0x3F]); // ACCEL_ZOUT_H
        vm.write_data(TWCR, START);
        vm.write_data(TWDR, (0x68 << 1) | 1);
        vm.write_data(TWCR, EN);
        vm.write_data(TWCR, EN);
        assert_eq!(vm.read_data(TWDR), 0x40, "1 g on Z = 0x4000, high byte 0x40");
    }
}
