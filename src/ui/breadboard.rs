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

// The breadboard: a visual workbench wired to the live simulator.
//
// Two kinds of things live here:
//   * GPIO components (LED, RGB LED, push button, 7-segment digit, LED bar) you
//     place on the Schematic tab and wire to individual pins. An output lights
//     when its pin's PORT bit is high *and* the pin is an output (DDR bit set);
//     a button drives its pin low while held (active-low, internal pull-up).
//   * On-chip serial buses (UART/SPI/I2C) shown on their own tabs, fed by the
//     bytes the program transmits — plus a UART send box and an SPI response.

use std::collections::HashMap;

use eframe::egui;

use crate::app::IkIdeApp;
use crate::core::board::{self, Pin};
use crate::core::devices::Bus;

/// The breadboard's top-level views.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BreadboardTab {
    Schematic,
    Uart,
    Spi,
    I2c,
}

impl Default for BreadboardTab {
    fn default() -> Self {
        BreadboardTab::Schematic
    }
}

/// A GPIO component kind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompKind {
    Led,
    RgbLed,
    Button,
    SevenSeg,
    LedBar,
    Potentiometer,
}

impl CompKind {
    fn label(self) -> &'static str {
        match self {
            CompKind::Led => "LED",
            CompKind::RgbLed => "RGB LED",
            CompKind::Button => "Button",
            CompKind::SevenSeg => "7-segment",
            CompKind::LedBar => "LED bar",
            CompKind::Potentiometer => "Potentiometer",
        }
    }
    /// Pin-terminal count (the LED bar and potentiometer are special-cased).
    fn fixed_terminals(self) -> usize {
        match self {
            CompKind::Led | CompKind::Button => 1,
            CompKind::RgbLed => 3,
            CompKind::SevenSeg => 8,
            CompKind::LedBar | CompKind::Potentiometer => 0,
        }
    }
}

/// One placed component. `pins[t]` is the MCU pin wired to terminal `t`.
pub struct Component {
    pub kind: CompKind,
    pub pins: Vec<Option<usize>>,
    pub color_idx: usize,
    pub pressed: bool,
    /// Potentiometer: the ADC channel and its 10-bit value.
    pub adc_channel: u8,
    pub adc_value: u16,
}

impl Component {
    fn new(kind: CompKind) -> Self {
        let n = if kind == CompKind::LedBar { 8 } else { kind.fixed_terminals() };
        Component {
            kind,
            pins: vec![None; n],
            color_idx: 0,
            pressed: false,
            adc_channel: 0,
            adc_value: 512,
        }
    }
}

/// The breadboard model, owned by the app for the session. Pins are cached per
/// target so we don't re-parse `gpio.ik` per frame.
#[derive(Default)]
pub struct Breadboard {
    pub components: Vec<Component>,
    cached_device: String,
    cached_pins: Vec<Pin>,
}

impl Breadboard {
    pub fn ensure_pins(&mut self, device: &str) {
        if self.cached_device != device {
            self.cached_device = device.to_string();
            self.cached_pins = board::pins(device);
        }
    }
    pub fn pins(&self) -> &[Pin] {
        &self.cached_pins
    }
}

/// LED colour palette: lit colour for each choice.
const LED_COLORS: &[(&str, egui::Color32)] = &[
    ("Red", egui::Color32::from_rgb(255, 70, 70)),
    ("Green", egui::Color32::from_rgb(80, 255, 110)),
    ("Blue", egui::Color32::from_rgb(90, 150, 255)),
    ("Yellow", egui::Color32::from_rgb(255, 220, 70)),
    ("White", egui::Color32::from_rgb(245, 245, 245)),
];

fn dim(c: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgb(
        (c.r() as f32 * 0.18) as u8 + 12,
        (c.g() as f32 * 0.18) as u8 + 12,
        (c.b() as f32 * 0.18) as u8 + 12,
    )
}

/// True when `pin` is configured as an output and currently driven high.
fn pin_high(regs: &HashMap<u32, u8>, pin: &Pin) -> bool {
    let port = *regs.get(&pin.port_addr).unwrap_or(&0);
    let ddr = *regs.get(&pin.ddr_addr).unwrap_or(&0);
    (port >> pin.bit) & 1 == 1 && (ddr >> pin.bit) & 1 == 1
}

/// Actions collected during the UI pass, applied once the model borrow ends.
#[derive(Default)]
struct Actions {
    close: bool,
    start: bool,
    stop: bool,
    add: Option<CompKind>,
    remove: Option<usize>,
    /// Pin inputs to drive (PINx addr, bit, level-high).
    inputs: Vec<(u32, u8, bool)>,
    uart_send: Option<Vec<u8>>,
    spi_miso: Option<u8>,
    /// ADC channel values to push (channel, value).
    adc: Vec<(u8, u16)>,
    /// Catalog id of a device to attach to the buses.
    attach: Option<String>,
    /// Index into bb_devices of a device to detach.
    detach: Option<usize>,
}

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    if !app.show_breadboard {
        return;
    }

    let device = app
        .active_tab
        .and_then(|i| app.open_tabs.get(i))
        .and_then(|t| board::target_of(&t.content));

    let mut act = Actions::default();

    egui::Window::new("breadboard")
        .title_bar(false)
        .resizable(true)
        .default_width(760.0)
        .default_height(480.0)
        .show(ctx, |ui| {
            // --- Header ---
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔌 Breadboard").strong());
                if let Some(dev) = &device {
                    ui.label(egui::RichText::new(format!("target: {}", dev)).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        act.close = true;
                    }
                });
            });
            ui.separator();

            // --- Run toolbar ---
            let live = app.live.is_some();
            ui.horizontal_wrapped(|ui| {
                if !live {
                    if ui
                        .add_enabled(device.is_some(), egui::Button::new("▶ Run"))
                        .on_hover_text("Compile the active file and run it live; components track the simulated I/O.")
                        .clicked()
                    {
                        act.start = true;
                    }
                } else {
                    if ui.button("⏹ Stop").clicked() {
                        act.stop = true;
                    }
                    let (txt, col) = if app.live_running {
                        ("● running", egui::Color32::from_rgb(80, 220, 110))
                    } else {
                        ("● halted", egui::Color32::GRAY)
                    };
                    ui.label(egui::RichText::new(format!("{}  ·  {} cyc", txt, app.live_cycles)).color(col));
                }
                ui.separator();
                ui.add_enabled_ui(!live, |ui| {
                    let mut mhz = app.bb_clock_hz as f32 / 1_000_000.0;
                    ui.label("Clock:");
                    if ui
                        .add(egui::DragValue::new(&mut mhz).speed(0.5).range(0.0..=32.0).suffix(" MHz"))
                        .on_hover_text("Assumed CPU clock used to pace the run to real time. Match your code's F_CPU; 0 = full speed.")
                        .changed()
                    {
                        app.bb_clock_hz = (mhz * 1_000_000.0) as u32;
                    }
                });
            });
            if !app.live_status.is_empty() {
                ui.label(egui::RichText::new(&app.live_status).weak().small());
            }

            // --- Tab bar ---
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.bb_tab, BreadboardTab::Schematic, "🧩 Schematic");
                ui.selectable_value(&mut app.bb_tab, BreadboardTab::Uart, "🖧 UART");
                ui.selectable_value(&mut app.bb_tab, BreadboardTab::Spi, "🔗 SPI");
                ui.selectable_value(&mut app.bb_tab, BreadboardTab::I2c, "🔗 I2C");
            });
            ui.separator();

            if device.is_none() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Open an .ik file with a `target` declaration to begin.").weak());
                });
                return;
            }
            let device = device.clone().unwrap();
            app.breadboard.ensure_pins(&device);

            match app.bb_tab {
                BreadboardTab::Schematic => schematic_tab(app, ui, &mut act),
                BreadboardTab::Uart => uart_tab(app, ui, &mut act),
                BreadboardTab::Spi => spi_tab(app, ui, &mut act),
                BreadboardTab::I2c => i2c_tab(app, ui, &mut act),
            }
        });

    apply_actions(app, act);
}

/// The Schematic tab: a palette plus the placed GPIO components.
fn schematic_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label("Add component:");
        let items: Vec<(CompKind, String)> = [
            CompKind::Led,
            CompKind::RgbLed,
            CompKind::Button,
            CompKind::SevenSeg,
            CompKind::LedBar,
            CompKind::Potentiometer,
        ]
        .into_iter()
        .map(|k| (k, k.label().to_string()))
        .collect();
        if let Some(kind) = crate::ui::widgets::filter_combo(ui, "bb_add_component", "➕ Choose…", &items) {
            act.add = Some(kind);
        }
    });
    ui.add_space(4.0);

    let pins: Vec<Pin> = app.breadboard.pins().to_vec();
    let n = app.breadboard.components.len();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Display devices render here even when there are no GPIO components.
        display_panel(app, ui);

        if n == 0 {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Add a component, or attach a device on the UART/SPI/I2C tabs.").weak());
            });
        }
        for i in 0..n {
            component_card(app, ui, &pins, i, act);
            ui.add_space(6.0);
        }
    });
}

/// Render the framebuffers of any attached display devices, re-uploading a
/// texture only when its contents changed.
fn display_panel(app: &mut IkIdeApp, ui: &mut egui::Ui) {
    // Display devices attached but not yet rendered (before Run / no framebuffer).
    let pending: Vec<String> = if app.bb_displays.is_empty() {
        app.bb_devices
            .iter()
            .filter_map(|id| {
                app.device_catalog
                    .iter()
                    .find(|s| &s.id == id && s.has_display)
                    .map(|s| s.name.clone())
            })
            .collect()
    } else {
        Vec::new()
    };

    if app.bb_displays.is_empty() && pending.is_empty() {
        return;
    }

    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new("Displays").strong());

    for name in &pending {
        ui.label(egui::RichText::new(format!("📺 {} — press Run to render", name)).weak());
    }

    for info in app.bb_displays.clone() {
        let snapshot = info.handle.0.lock().ok().map(|fb| (fb.w, fb.h, fb.generation, fb.pixels.clone()));
        let (w, h, generation, pixels) = match snapshot {
            Some(s) if s.0 > 0 => s,
            _ => continue,
        };

        let stale = app.bb_textures.get(&info.name).map(|(g, _)| *g != generation).unwrap_or(true);
        if stale {
            let mut img = egui::ColorImage::new([w, h], egui::Color32::BLACK);
            for (i, p) in pixels.iter().enumerate() {
                img.pixels[i] = egui::Color32::from_rgb((p >> 16) as u8, (p >> 8) as u8, *p as u8);
            }
            let handle = ui.ctx().load_texture(
                format!("bbdisp_{}", info.name),
                img,
                egui::TextureOptions::NEAREST,
            );
            app.bb_textures.insert(info.name.clone(), (generation, handle));
        }

        if let Some((_, tex)) = app.bb_textures.get(&info.name) {
            ui.label(egui::RichText::new(format!("{}  ({}×{})", info.name, w, h)).weak().small());
            let scale = (240.0 / w as f32).min(2.0);
            let size = egui::vec2(w as f32 * scale, h as f32 * scale);
            // Draw the panel, then outline exactly its rect (it's often mostly
            // black, so a border makes its extent visible).
            let resp = ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), size)));
            ui.painter().rect_stroke(
                resp.rect,
                egui::Rounding::ZERO,
                egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
            );
        }
    }
}

/// One component as a card: live visual on the left, pin wiring on the right.
fn component_card(app: &mut IkIdeApp, ui: &mut egui::Ui, pins: &[Pin], i: usize, act: &mut Actions) {
    let kind = app.breadboard.components[i].kind;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            // --- Live visual ---
            draw_visual(app, ui, pins, i);

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // --- Controls ---
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(kind.label()).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").on_hover_text("Remove").clicked() {
                            act.remove = Some(i);
                        }
                    });
                });

                // LED-family colour picker.
                if matches!(kind, CompKind::Led | CompKind::LedBar) {
                    let cidx = app.breadboard.components[i].color_idx.min(LED_COLORS.len() - 1);
                    egui::ComboBox::from_id_salt(("bb_col", i))
                        .selected_text(LED_COLORS[cidx].0)
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (ci, (name, _)) in LED_COLORS.iter().enumerate() {
                                ui.selectable_value(&mut app.breadboard.components[i].color_idx, ci, *name);
                            }
                        });
                }

                // LED bar length.
                if kind == CompKind::LedBar {
                    let mut len = app.breadboard.components[i].pins.len();
                    if ui
                        .add(egui::DragValue::new(&mut len).range(1..=16).prefix("LEDs: "))
                        .changed()
                    {
                        app.breadboard.components[i].pins.resize(len, None);
                    }
                }

                // Button press (momentary).
                if kind == CompKind::Button {
                    let b = ui.add(egui::Button::new("⏺ hold").min_size(egui::vec2(64.0, 28.0)));
                    app.breadboard.components[i].pressed = b.is_pointer_button_down_on();
                }

                // Potentiometer: ADC channel + analog value.
                if kind == CompKind::Potentiometer {
                    ui.horizontal(|ui| {
                        ui.label("ADC ch:");
                        egui::ComboBox::from_id_salt(("bb_adc_ch", i))
                            .selected_text(format!("{}", app.breadboard.components[i].adc_channel))
                            .width(48.0)
                            .show_ui(ui, |ui| {
                                for ch in 0u8..8 {
                                    ui.selectable_value(&mut app.breadboard.components[i].adc_channel, ch, format!("{}", ch));
                                }
                            });
                    });
                    let mut v = app.breadboard.components[i].adc_value;
                    if ui.add(egui::Slider::new(&mut v, 0..=1023).text("value")).changed() {
                        app.breadboard.components[i].adc_value = v;
                    }
                }

                // Terminal → pin selectors.
                let labels = terminal_labels(kind, app.breadboard.components[i].pins.len());
                ui.horizontal_wrapped(|ui| {
                    let count = app.breadboard.components[i].pins.len();
                    for t in 0..count {
                        pin_selector(ui, pins, &mut app.breadboard.components[i].pins[t], i, t, &labels[t]);
                    }
                });
            });
        });
    });

    // Queue button input (active-low w/ pull-up) when live.
    if kind == CompKind::Button && app.live.is_some() {
        let pressed = app.breadboard.components[i].pressed;
        if let Some(pin) = app.breadboard.components[i].pins[0].and_then(|p| pins.get(p)) {
            act.inputs.push((pin.pin_addr, pin.bit, !pressed));
        }
    }
    // Push the potentiometer's analog value to its ADC channel when live.
    if kind == CompKind::Potentiometer && app.live.is_some() {
        let c = &app.breadboard.components[i];
        act.adc.push((c.adc_channel, c.adc_value));
    }
}

/// Per-terminal labels for a component's pin selectors.
fn terminal_labels(kind: CompKind, count: usize) -> Vec<String> {
    match kind {
        CompKind::Led | CompKind::Button => vec!["pin".to_string()],
        CompKind::RgbLed => vec!["R".to_string(), "G".to_string(), "B".to_string()],
        CompKind::SevenSeg => ["a", "b", "c", "d", "e", "f", "g", "dp"].iter().map(|s| s.to_string()).collect(),
        CompKind::LedBar => (0..count).map(|n| n.to_string()).collect(),
        CompKind::Potentiometer => Vec::new(),
    }
}

/// A compact "label: [pin ▾]" selector bound to one terminal. The dropdown is
/// searchable so it stays usable on chips with dozens of pins.
fn pin_selector(ui: &mut egui::Ui, pins: &[Pin], sel: &mut Option<usize>, i: usize, t: usize, label: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).small());
        let cur = sel.and_then(|p| pins.get(p)).map(|p| p.name.clone()).unwrap_or_else(|| "—".to_string());
        let mut items: Vec<(Option<usize>, String)> = Vec::with_capacity(pins.len() + 1);
        items.push((None, "— none —".to_string()));
        for (pi, p) in pins.iter().enumerate() {
            items.push((Some(pi), p.name.clone()));
        }
        if let Some(v) = crate::ui::widgets::filter_combo(ui, ("bb_pin", i, t), &cur, &items) {
            *sel = v;
        }
    });
}

/// Draw a component's live visual via a small painter.
fn draw_visual(app: &IkIdeApp, ui: &mut egui::Ui, pins: &[Pin], i: usize) {
    let comp = &app.breadboard.components[i];
    let regs = &app.live_regs;
    let live = app.live.is_some();
    let on = |t: usize| -> bool {
        live && comp.pins.get(t).copied().flatten().and_then(|p| pins.get(p)).map(|p| pin_high(regs, p)).unwrap_or(false)
    };
    let color = LED_COLORS[comp.color_idx.min(LED_COLORS.len() - 1)].1;

    match comp.kind {
        CompKind::Led => {
            let (_r, p) = ui.allocate_painter(egui::vec2(40.0, 40.0), egui::Sense::hover());
            let c = if on(0) { color } else { dim(color) };
            p.circle_filled(_r.rect.center(), 14.0, c);
        }
        CompKind::Button => {
            let (_r, p) = ui.allocate_painter(egui::vec2(40.0, 40.0), egui::Sense::hover());
            let c = if comp.pressed { egui::Color32::from_rgb(120, 200, 120) } else { egui::Color32::from_gray(90) };
            p.rect_filled(egui::Rect::from_center_size(_r.rect.center(), egui::vec2(24.0, 24.0)), egui::Rounding::same(4.0), c);
        }
        CompKind::RgbLed => {
            let (_r, p) = ui.allocate_painter(egui::vec2(40.0, 40.0), egui::Sense::hover());
            let c = egui::Color32::from_rgb(
                if on(0) { 255 } else { 18 },
                if on(1) { 255 } else { 18 },
                if on(2) { 255 } else { 18 },
            );
            p.circle_filled(_r.rect.center(), 14.0, c);
        }
        CompKind::SevenSeg => {
            let (_r, p) = ui.allocate_painter(egui::vec2(46.0, 64.0), egui::Sense::hover());
            draw_seven_seg(&p, _r.rect, color, &(0..7).map(|t| on(t)).collect::<Vec<_>>(), on(7));
        }
        CompKind::LedBar => {
            let count = comp.pins.len();
            let (_r, p) = ui.allocate_painter(egui::vec2((count as f32 * 16.0).max(16.0), 24.0), egui::Sense::hover());
            for t in 0..count {
                let cx = _r.rect.left() + 8.0 + t as f32 * 16.0;
                let c = if on(t) { color } else { dim(color) };
                p.circle_filled(egui::pos2(cx, _r.rect.center().y), 6.0, c);
            }
        }
        CompKind::Potentiometer => {
            // A fill bar proportional to the 0..1023 value.
            let (_r, p) = ui.allocate_painter(egui::vec2(40.0, 40.0), egui::Sense::hover());
            let frac = comp.adc_value as f32 / 1023.0;
            let r = _r.rect.shrink(6.0);
            p.rect_stroke(r, egui::Rounding::same(3.0), egui::Stroke::new(1.0, egui::Color32::from_gray(110)));
            let fill = egui::Rect::from_min_max(
                egui::pos2(r.left(), r.bottom() - r.height() * frac),
                r.right_bottom(),
            );
            p.rect_filled(fill, egui::Rounding::same(3.0), egui::Color32::from_rgb(110, 170, 255));
        }
    }
}

/// Render a 7-segment digit. `seg[0..7]` are segments a..g; `dp` is the dot.
fn draw_seven_seg(p: &egui::Painter, rect: egui::Rect, color: egui::Color32, seg: &[bool], dp: bool) {
    let on = color;
    let off = dim(color);
    let w = 26.0;
    let h = 48.0;
    let x0 = rect.center().x - w / 2.0;
    let y0 = rect.top() + 6.0;
    let t = 4.0; // segment thickness
    let seg_color = |i: usize| if seg.get(i).copied().unwrap_or(false) { on } else { off };
    let hbar = |p: &egui::Painter, x: f32, y: f32, col: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(x + t, y - t / 2.0), egui::vec2(w - 2.0 * t, t)), egui::Rounding::same(1.0), col);
    };
    let vbar = |p: &egui::Painter, x: f32, y: f32, col: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(x - t / 2.0, y + t), egui::vec2(t, h / 2.0 - 2.0 * t)), egui::Rounding::same(1.0), col);
    };
    // a top, b top-right, c bottom-right, d bottom, e bottom-left, f top-left, g middle
    hbar(p, x0, y0, seg_color(0));
    vbar(p, x0 + w, y0, seg_color(1));
    vbar(p, x0 + w, y0 + h / 2.0, seg_color(2));
    hbar(p, x0, y0 + h, seg_color(3));
    vbar(p, x0, y0 + h / 2.0, seg_color(4));
    vbar(p, x0, y0, seg_color(5));
    hbar(p, x0, y0 + h / 2.0, seg_color(6));
    p.circle_filled(egui::pos2(x0 + w + 6.0, y0 + h), 2.5, if dp { on } else { off });
}

/// Attached devices for one bus, with detach buttons and a searchable picker.
fn device_panel(app: &mut IkIdeApp, ui: &mut egui::Ui, bus: Bus, act: &mut Actions) {
    ui.label(egui::RichText::new("Devices on this bus").strong());
    let mut any = false;
    for (i, id) in app.bb_devices.iter().enumerate() {
        if let Some(spec) = app.device_catalog.iter().find(|s| &s.id == id) {
            if spec.bus == bus {
                any = true;
                let addr = spec.address.map(|a| format!(" @0x{:02X}", a)).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(format!("• {}{}", spec.name, addr));
                    if ui.small_button("✖").clicked() {
                        act.detach = Some(i);
                    }
                });
            }
        }
    }
    if !any {
        ui.label(egui::RichText::new("none attached").weak().small());
    }
    let items: Vec<(String, String)> = app
        .device_catalog
        .iter()
        .filter(|s| s.bus == bus)
        .map(|s| {
            let addr = s.address.map(|a| format!(" @0x{:02X}", a)).unwrap_or_default();
            (s.id.clone(), format!("{}{}", s.name, addr))
        })
        .collect();
    ui.add_enabled_ui(app.live.is_none(), |ui| {
        if let Some(id) = crate::ui::widgets::filter_combo(ui, ("attach_dev", bus.label()), "➕ Attach device…", &items) {
            act.attach = Some(id);
        }
    });
    if app.live.is_some() {
        ui.label(egui::RichText::new("Device changes apply on the next Run.").weak().small());
    }
    ui.separator();
}

/// The UART tab: transmitted-text console plus a best-effort send box.
fn uart_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    if board::periph_addrs(&app.breadboard.cached_device_name()).uart.is_none() {
        unavailable(ui, "USART isn't modeled for this target in the simulator.");
        return;
    }
    device_panel(app, ui, Bus::Uart, act);

    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.bb_uart_plot, false, "Console");
        ui.selectable_value(&mut app.bb_uart_plot, true, "Plotter");
    });
    ui.separator();

    if app.bb_uart_plot {
        // Same plotter the Serial Monitor uses, fed by the UART transcript.
        crate::ui::serial::render_plotter(app, ui);
        return;
    }

    ui.label(egui::RichText::new("Text the program transmits (TX).").weak().small());
    log_view(ui, &app.uart_log, "uart_log");

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let send = ui.add(egui::TextEdit::singleline(&mut app.uart_send).desired_width(200.0).hint_text("type to send to the program…"));
        let enter = send.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui.add_enabled(app.live.is_some(), egui::Button::new("Send")).clicked();
        if (enter || clicked) && !app.uart_send.is_empty() {
            let mut bytes = app.uart_send.clone().into_bytes();
            bytes.push(b'\n');
            act.uart_send = Some(bytes);
            app.uart_send.clear();
        }
    });
    ui.label(egui::RichText::new("Bytes typed here are delivered to the program's receiver.").weak().small());
}

/// The SPI tab: transmitted bytes plus the configurable read-back (MISO) byte.
fn spi_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    if board::periph_addrs(&app.breadboard.cached_device_name()).spi.is_none() {
        unavailable(ui, "This target has no SPI.");
        return;
    }
    device_panel(app, ui, Bus::Spi, act);
    ui.label(egui::RichText::new("Bytes the master transmits (MOSI), in hex.").weak().small());
    log_view(ui, &app.spi_log, "spi_log");

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("MISO response:");
        let mut v = app.bb_spi_miso;
        if ui
            .add(egui::DragValue::new(&mut v).hexadecimal(2, false, true).prefix("0x"))
            .on_hover_text("Byte returned to the master on each transfer (read back from the data register).")
            .changed()
        {
            app.bb_spi_miso = v;
            act.spi_miso = Some(v);
        }
    });
}

/// The I2C tab: attached devices plus the decoded TWI bus transcript.
fn i2c_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    device_panel(app, ui, Bus::I2c, act);
    ui.label(
        egui::RichText::new("TWI bus: [S] start, addr 0xNN R/W, data bytes (hex), [P] stop.")
            .weak()
            .small(),
    );
    log_view(ui, &app.twi_log, "twi_log");
}

fn unavailable(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(msg).weak());
    });
}

/// A scrolling monospace transcript that sticks to the bottom.
fn log_view(ui: &mut egui::Ui, text: &str, id: &str) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .max_height(220.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(text).monospace());
        });
}

/// Apply the actions gathered during the UI pass.
fn apply_actions(app: &mut IkIdeApp, act: Actions) {
    if let Some(idx) = act.remove {
        if idx < app.breadboard.components.len() {
            app.breadboard.components.remove(idx);
        }
    }
    if let Some(idx) = act.detach {
        if idx < app.bb_devices.len() {
            app.bb_devices.remove(idx);
        }
    }
    if let Some(id) = act.attach {
        app.bb_devices.push(id);
    }
    if let Some(kind) = act.add {
        app.breadboard.components.push(Component::new(kind));
    }
    if let Some(live) = &app.live {
        for (addr, bit, high) in act.inputs {
            live.set_input(addr, bit, high);
        }
        if let Some(bytes) = act.uart_send {
            live.uart_send(bytes);
        }
        if let Some(b) = act.spi_miso {
            live.set_spi_miso(b);
        }
        for (ch, val) in act.adc {
            live.set_adc(ch, val);
        }
    }
    if act.start {
        app.start_breadboard();
    }
    if act.stop {
        app.stop_breadboard();
    }
    if act.close {
        app.show_breadboard = false;
    }
}

impl Breadboard {
    /// The device whose pins are currently cached (for peripheral lookups).
    pub fn cached_device_name(&self) -> String {
        self.cached_device.clone()
    }
}
