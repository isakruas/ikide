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

// The breadboard: a virtual board built from scripted devices.
//
// Every part is an instance of a catalog device (a Rhai script). The card UI
// is generated from the device's declared interface: its `view` elements are
// rendered live (LEDs from pin state or script state, buttons driving pins,
// sliders feeding ADC channels, framebuffer displays) and its `pins` become
// wiring selectors. The Schematic tab is the board; the UART/SPI/I2C tabs are
// bus monitors.

use std::collections::HashMap;

use eframe::egui;

use crate::app::IkIdeApp;
use crate::core::board::{self, Pin};
use crate::core::devices::{Bus, DeviceSpec, InstanceWiring, PinMode, ViewDef, ViewKind, ViewVal};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BreadboardTab {
    #[default]
    Schematic,
    Uart,
    Spi,
    I2c,
}

/// One placed device: which catalog entry, and how its terminals are wired.
pub struct Instance {
    pub spec_id: String,
    /// Per-terminal wiring, parallel to the spec's `pins` (index into the
    /// board pin list for watch/drive terminals; ADC terminals use `adc_ch`).
    pub pins: Vec<Option<usize>>,
    pub adc_ch: Vec<u8>,
    /// Transient UI state: slider values and held buttons, keyed by element.
    pub slider_vals: HashMap<usize, f32>,
    pub pressed: HashMap<usize, bool>,
}

impl Instance {
    pub fn new(spec: &DeviceSpec, board_pins: &[Pin]) -> Self {
        // Pre-wire terminals that declare a default pin.
        let pins = spec
            .pins
            .iter()
            .map(|def| {
                def.default_pin
                    .as_ref()
                    .and_then(|n| board_pins.iter().position(|p| &p.name == n))
            })
            .collect();
        Instance {
            spec_id: spec.id.clone(),
            pins,
            adc_ch: vec![0; spec.pins.len()],
            slider_vals: HashMap::new(),
            pressed: HashMap::new(),
        }
    }

    /// Resolve this instance to the wiring the engine builds from.
    pub fn wiring(&self, catalog: &[DeviceSpec], board_pins: &[Pin]) -> InstanceWiring {
        let mut w = InstanceWiring { spec_id: self.spec_id.clone(), ..Default::default() };
        if let Some(spec) = catalog.iter().find(|s| s.id == self.spec_id) {
            for (t, def) in spec.pins.iter().enumerate() {
                match def.mode {
                    PinMode::Adc => {
                        w.adc.insert(def.name.clone(), self.adc_ch.get(t).copied().unwrap_or(0));
                    }
                    _ => {
                        if let Some(p) = self.pins.get(t).copied().flatten().and_then(|i| board_pins.get(i)) {
                            w.pins.insert(def.name.clone(), p.name.clone());
                        }
                    }
                }
            }
        }
        w
    }
}

/// The board model: placed instances plus the cached pin map of the target.
#[derive(Default)]
pub struct Breadboard {
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
    pub fn device_name(&self) -> &str {
        &self.cached_device
    }
}

fn dim(c: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgb(
        (c.r() as f32 * 0.18) as u8 + 12,
        (c.g() as f32 * 0.18) as u8 + 12,
        (c.b() as f32 * 0.18) as u8 + 12,
    )
}

fn rgb(c: u32) -> egui::Color32 {
    egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// True when `pin` is configured as an output and driven high.
fn pin_high(regs: &HashMap<u32, u8>, pin: &Pin) -> bool {
    let port = *regs.get(&pin.port_addr).unwrap_or(&0);
    let ddr = *regs.get(&pin.ddr_addr).unwrap_or(&0);
    (port >> pin.bit) & 1 == 1 && (ddr >> pin.bit) & 1 == 1
}

/// Actions collected during the UI pass, applied after the borrow ends.
#[derive(Default)]
struct Actions {
    close: bool,
    start: bool,
    stop: bool,
    add: Option<String>,
    remove: Option<usize>,
    refresh_catalog: bool,
    new_script: bool,
    /// (PINx addr, bit, level) inputs from pin-bound buttons.
    inputs: Vec<(u32, u8, bool)>,
    /// ADC values from pin-bound sliders.
    adc: Vec<(u8, u16)>,
    /// Script events from id-bound elements.
    view_events: Vec<(usize, String, f64)>,
    uart_send: Option<Vec<u8>>,
    spi_miso: Option<u8>,
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
        .default_width(780.0)
        .default_height(520.0)
        .show(ctx, |ui| {
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

            let live = app.live.is_some();
            ui.horizontal_wrapped(|ui| {
                if !live {
                    if ui
                        .add_enabled(device.is_some(), egui::Button::new("▶ Run"))
                        .on_hover_text("Compile the active file and run it against the virtual board.")
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
                    let stack_bytes = (app.live_sram_start + app.live_sram_bytes)
                        .saturating_sub(1)
                        .saturating_sub(app.live_sp as u32);
                    let status_text = format!(
                        "{}  ·  {} cyc  ·  SP: 0x{:04X} (Stack: {} B)",
                        txt,
                        app.live_cycles,
                        app.live_sp,
                        stack_bytes
                    );
                    ui.label(egui::RichText::new(status_text).color(col));
                    ui.checkbox(&mut app.show_vm_registers, "Registers");
                }
                ui.separator();
                ui.add_enabled_ui(!live, |ui| {
                    let mut mhz = app.bb_clock_hz as f32 / 1_000_000.0;
                    ui.label("Clock:");
                    if ui
                        .add(egui::DragValue::new(&mut mhz).speed(0.5).range(0.0..=32.0).suffix(" MHz"))
                        .on_hover_text("Assumed CPU clock pacing the run. Match @cpu_mhz(); 0 = full speed.")
                        .changed()
                    {
                        app.bb_clock_hz = (mhz * 1_000_000.0) as u32;
                    }
                });
            });
            if live && app.show_vm_registers {
                ui.add_space(4.0);
                egui::Frame::none()
                    .fill(ui.style().visuals.widgets.noninteractive.bg_fill)
                    .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.columns(2, |columns| {
                            // Left column: Registers
                            columns[0].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.monospace(format!("SREG: 0x{:02X}", app.live_sreg));
                                    ui.separator();
                                    ui.monospace(format!("PC: 0x{:06X}", app.live_pc));
                                    ui.separator();
                                    ui.monospace(format!("SRAM: 0x{:04X}-0x{:04X}", 
                                        app.live_sram_start, 
                                        app.live_sram_start + app.live_sram_bytes - 1
                                    ));
                                });
                                ui.add_space(4.0);
                                egui::Grid::new("reg_grid").striped(true).show(ui, |ui| {
                                    for row in 0..8 {
                                        for col in 0..4 {
                                            let reg_idx = row * 4 + col;
                                            ui.monospace(format!("r{:02}: 0x{:02X}  ", reg_idx, app.live_r[reg_idx]));
                                        }
                                        ui.end_row();
                                    }
                                });
                            });

                            // Right column: RAM usage history chart
                            columns[1].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong("📈 SRAM Usage (Static + Stack - 60s)");
                                });
                                ui.add_space(4.0);

                                let history = &app.ram_history;
                                let max_val = history.iter().copied().max().unwrap_or(0);
                                let last_val = history.last().copied().unwrap_or(0);

                                // Graph y-axis limit: 10% headroom, at least 32 bytes
                                let max_y = (max_val as f32 * 1.1).max(32.0);

                                // Allocate space for the chart
                                let (rect, _response) = ui.allocate_at_least(
                                    egui::vec2(ui.available_width().max(150.0), 100.0),
                                    egui::Sense::hover(),
                                );

                                // Draw background and border
                                let painter = ui.painter_at(rect);
                                painter.rect(
                                    rect,
                                    4.0,
                                    egui::Color32::from_black_alpha(30),
                                    egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color.linear_multiply(0.5)),
                                );

                                // Draw gridlines (middle line)
                                let stroke_grid = egui::Stroke::new(
                                    1.0,
                                    ui.visuals().widgets.noninteractive.bg_stroke.color.linear_multiply(0.2),
                                );
                                let mid_y = rect.center().y;
                                painter.line_segment(
                                    [egui::pos2(rect.left(), mid_y), egui::pos2(rect.right(), mid_y)],
                                    stroke_grid,
                                );

                                // Draw Y-axis labels
                                let font_id = egui::FontId::monospace(9.0);
                                let text_color = ui.visuals().text_color().linear_multiply(0.5);
                                painter.text(
                                    egui::pos2(rect.left() + 6.0, rect.top() + 6.0),
                                    egui::Align2::LEFT_TOP,
                                    format!("{:.0} B", max_y),
                                    font_id.clone(),
                                    text_color,
                                );
                                painter.text(
                                    egui::pos2(rect.left() + 6.0, mid_y),
                                    egui::Align2::LEFT_CENTER,
                                    format!("{:.0} B", max_y / 2.0),
                                    font_id.clone(),
                                    text_color,
                                );
                                painter.text(
                                    egui::pos2(rect.left() + 6.0, rect.bottom() - 6.0),
                                    egui::Align2::LEFT_BOTTOM,
                                    "0 B",
                                    font_id.clone(),
                                    text_color,
                                );

                                // Draw plot lines if history has at least 2 points
                                if history.len() >= 2 {
                                    let max_samples = 3600.0;
                                    let visible_ratio = history.len() as f32 / max_samples;
                                    let visible_width = visible_ratio * rect.width();
                                    let num_bins = (visible_width.round() as usize).max(2);

                                    let mut points = Vec::with_capacity(num_bins);
                                    for col in 0..num_bins {
                                        let history_idx = (col * (history.len() - 1)) / (num_bins - 1);
                                        let val = history[history_idx];

                                        let offset_ratio = (history.len() - 1 - history_idx) as f32 / (max_samples - 1.0);
                                        let x = rect.right() - offset_ratio * rect.width();
                                        let y = rect.bottom()
                                            - ((val as f32 / max_y) * rect.height()).min(rect.height());
                                        points.push(egui::pos2(x, y));
                                    }

                                    // Draw the area fill using vertical line segments for each binned column
                                    // This avoids any triangulation flickering bugs with concave polygons!
                                    let fill_color = egui::Color32::from_rgba_unmultiplied(0, 180, 255, 15);
                                    for p in &points {
                                        painter.line_segment(
                                            [egui::pos2(p.x, rect.bottom()), *p],
                                            egui::Stroke::new(1.0, fill_color),
                                        );
                                    }

                                    // Plot line
                                    painter.add(egui::Shape::line(
                                        points,
                                        egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 180, 255)),
                                    ));
                                } else if let Some(&val) = history.first() {
                                    // Single point: draw a dot on the right edge
                                    let y = rect.bottom() - ((val as f32 / max_y) * rect.height()).min(rect.height());
                                    painter.circle_filled(
                                        egui::pos2(rect.right(), y),
                                        2.0,
                                        egui::Color32::from_rgb(0, 180, 255),
                                    );
                                }

                                ui.add_space(4.0);

                                // Stats row
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("Cur: {}B", last_val)).small().monospace());
                                    ui.separator();
                                    ui.label(egui::RichText::new(format!("Static: {}B", app.live_sram_static)).small().monospace());
                                    ui.separator();
                                    ui.label(egui::RichText::new(format!("Stack: {}B", last_val.saturating_sub(app.live_sram_static))).small().monospace());
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!("Peak: {}B", app.live_sram_peak))
                                            .small()
                                            .monospace()
                                            .strong()
                                            .color(egui::Color32::from_rgb(255, 165, 0)),
                                    );
                                });
                            });
                        });
                    });
            }
            if !app.live_status.is_empty() {
                ui.label(egui::RichText::new(&app.live_status).weak().small());
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.bb_tab, BreadboardTab::Schematic, "🧩 Board");
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
            app.breadboard.ensure_pins(&device.unwrap());

            match app.bb_tab {
                BreadboardTab::Schematic => board_tab(app, ui, &mut act),
                BreadboardTab::Uart => uart_tab(app, ui, &mut act),
                BreadboardTab::Spi => spi_tab(app, ui, &mut act),
                BreadboardTab::I2c => i2c_tab(app, ui),
            }
        });

    apply_actions(app, act);
}

// ---------------------------------------------------------------------------
// Board tab
// ---------------------------------------------------------------------------

fn board_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label("Add device:");
        let items: Vec<(String, String)> = app
            .device_catalog
            .iter()
            .map(|s| {
                let tag = match s.bus {
                    Bus::None => String::new(),
                    b => format!("  [{}]", b.label()),
                };
                (s.id.clone(), format!("{}{}", s.name, tag))
            })
            .collect();
        if let Some(id) = crate::ui::widgets::filter_combo(ui, "bb_add_device", "➕ Choose…", &items) {
            act.add = Some(id);
        }
        ui.add_space(8.0);
        if ui
            .button("📝 New device script")
            .on_hover_text("Create devices/my_device.rhai in the project and open it. It appears in the catalog after a refresh.")
            .clicked()
        {
            act.new_script = true;
        }
        if ui.button("⟳").on_hover_text("Reload the device catalog (built-ins + devices/*.rhai).").clicked() {
            act.refresh_catalog = true;
        }
    });
    ui.add_space(4.0);

    let pins: Vec<Pin> = app.breadboard.pins().to_vec();
    let n = app.bb_instances.len();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        display_panel(app, ui);
        if n == 0 {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Add a device — components and bus peripherals all live here.").weak());
            });
        }
        for i in 0..n {
            instance_card(app, ui, &pins, i, act);
            ui.add_space(6.0);
        }
    });
}

/// One placed device: header, generated view elements, wiring selectors.
fn instance_card(app: &mut IkIdeApp, ui: &mut egui::Ui, pins: &[Pin], i: usize, act: &mut Actions) {
    let spec = match app.device_catalog.iter().find(|s| s.id == app.bb_instances[i].spec_id) {
        Some(s) => s.clone(),
        None => return,
    };

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&spec.name).strong());
            if spec.bus != Bus::None {
                let addr = spec.address.map(|a| format!(" @0x{:02X}", a)).unwrap_or_default();
                ui.label(egui::RichText::new(format!("[{}{}]", spec.bus.label(), addr)).weak().small());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✖").on_hover_text("Remove").clicked() {
                    act.remove = Some(i);
                }
            });
        });

        // View elements, side by side.
        if !spec.view.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (e, view) in spec.view.iter().enumerate() {
                    render_view_element(app, ui, pins, &spec, i, e, view, act);
                }
            });
        }

        // Wiring selectors for every declared terminal.
        if !spec.pins.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (t, def) in spec.pins.iter().enumerate() {
                    match def.mode {
                        PinMode::Adc => {
                            ui.label(egui::RichText::new(format!("{}:", def.name)).small());
                            let ch = app.bb_instances[i].adc_ch.get(t).copied().unwrap_or(0);
                            egui::ComboBox::from_id_salt(("bb_adc", i, t))
                                .selected_text(format!("ADC{}", ch))
                                .width(70.0)
                                .show_ui(ui, |ui| {
                                    for c in 0u8..8 {
                                        ui.selectable_value(&mut app.bb_instances[i].adc_ch[t], c, format!("ADC{}", c));
                                    }
                                });
                        }
                        _ => {
                            ui.label(egui::RichText::new(format!("{}:", def.name)).small());
                            let cur = app.bb_instances[i].pins[t]
                                .and_then(|p| pins.get(p))
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "—".into());
                            let mut items: Vec<(Option<usize>, String)> = Vec::with_capacity(pins.len() + 1);
                            items.push((None, "— none —".into()));
                            items.extend(pins.iter().enumerate().map(|(pi, p)| (Some(pi), p.name.clone())));
                            if let Some(v) = crate::ui::widgets::filter_combo(ui, ("bb_pin", i, t), &cur, &items) {
                                app.bb_instances[i].pins[t] = v;
                            }
                        }
                    }
                }
            });
        }
    });
}

/// Render one declared view element and queue any input it produces.
#[allow(clippy::too_many_arguments)]
fn render_view_element(
    app: &mut IkIdeApp,
    ui: &mut egui::Ui,
    pins: &[Pin],
    spec: &DeviceSpec,
    i: usize,
    e: usize,
    view: &ViewDef,
    act: &mut Actions,
) {
    let live = app.live.is_some();
    // Resolve the element's terminal bindings up front, so the interactive
    // arms below are free to mutate the instance.
    let wired: Vec<Option<Pin>> = view
        .pins
        .iter()
        .map(|name| {
            spec.pins
                .iter()
                .position(|d| &d.name == name)
                .and_then(|t| app.bb_instances[i].pins.get(t).copied().flatten())
                .and_then(|p| pins.get(p).cloned())
        })
        .collect();
    let id_num: Option<f64> = view.id.as_ref().and_then(|id| match app.live_view.get(&(i, id.clone())) {
        Some(ViewVal::Num(n)) => Some(*n),
        _ => None,
    });
    let pin_on = |n: usize| -> bool {
        live && wired.get(n).and_then(|p| p.as_ref()).map(|p| pin_high(&app.live_regs, p)).unwrap_or(false)
    };
    let bit_on = |n: usize| -> bool {
        match (&view.id, id_num) {
            (Some(_), Some(v)) => (v as i64 >> n) & 1 == 1,
            (Some(_), None) => false,
            (None, _) => pin_on(n),
        }
    };

    match view.kind {
        ViewKind::Led => {
            let (r, p) = ui.allocate_painter(egui::vec2(36.0, 36.0), egui::Sense::hover());
            let c = if bit_on(0) { rgb(view.color) } else { dim(rgb(view.color)) };
            p.circle_filled(r.rect.center(), 13.0, c);
        }
        ViewKind::RgbLed => {
            let (r, p) = ui.allocate_painter(egui::vec2(36.0, 36.0), egui::Sense::hover());
            let c = egui::Color32::from_rgb(
                if pin_on(0) { 255 } else { 18 },
                if pin_on(1) { 255 } else { 18 },
                if pin_on(2) { 255 } else { 18 },
            );
            p.circle_filled(r.rect.center(), 13.0, c);
        }
        ViewKind::LedBar => {
            let count = if view.id.is_some() { view.bits } else { view.pins.len() };
            let (r, p) = ui.allocate_painter(egui::vec2((count as f32 * 16.0).max(16.0), 28.0), egui::Sense::hover());
            for n in 0..count {
                let cx = r.rect.left() + 8.0 + n as f32 * 16.0;
                let c = if bit_on(n) { rgb(view.color) } else { dim(rgb(view.color)) };
                p.circle_filled(egui::pos2(cx, r.rect.center().y), 6.0, c);
            }
        }
        ViewKind::SevenSeg => {
            let (r, p) = ui.allocate_painter(egui::vec2(46.0, 64.0), egui::Sense::hover());
            let seg: Vec<bool> = (0..7).map(|n| bit_on(n)).collect();
            draw_seven_seg(&p, r.rect, rgb(view.color), &seg, bit_on(7));
        }
        ViewKind::Button => {
            let label = view.label.as_deref().unwrap_or("hold");
            let b = ui.add(egui::Button::new(format!("⏺ {}", label)).min_size(egui::vec2(64.0, 28.0)));
            let down = b.is_pointer_button_down_on();
            let was = app.bb_instances[i].pressed.get(&e).copied().unwrap_or(false);
            app.bb_instances[i].pressed.insert(e, down);
            if live {
                if let Some(id) = &view.id {
                    if down != was {
                        act.view_events.push((i, id.clone(), if down { 1.0 } else { 0.0 }));
                    }
                } else if let Some(pin) = wired.first().and_then(|p| p.as_ref()) {
                    // Pressed -> the `press` level; released -> its inverse.
                    let level = if down { view.press != 0 } else { view.press == 0 };
                    act.inputs.push((pin.pin_addr, pin.bit, level));
                }
            }
        }
        ViewKind::Slider => {
            let mut v = app.bb_instances[i]
                .slider_vals
                .get(&e)
                .copied()
                .unwrap_or(((view.min + view.max) / 2.0) as f32);
            if ui
                .add(egui::Slider::new(&mut v, view.min as f32..=view.max as f32))
                .changed()
            {
                app.bb_instances[i].slider_vals.insert(e, v);
                if live {
                    if let Some(id) = &view.id {
                        act.view_events.push((i, id.clone(), v as f64));
                    }
                }
            } else {
                app.bb_instances[i].slider_vals.insert(e, v);
            }
            // ADC-bound sliders push their value continuously while live.
            if live && view.id.is_none() {
                if let Some(t) = view.pins.first().and_then(|n| spec.pins.iter().position(|d| &d.name == n)) {
                    if matches!(spec.pins[t].mode, PinMode::Adc) {
                        let ch = app.bb_instances[i].adc_ch.get(t).copied().unwrap_or(0);
                        act.adc.push((ch, v as u16));
                    }
                }
            }
        }
        ViewKind::Text => {
            let txt = match view.id.as_deref().map(|id| app.live_view.get(&(i, id.to_string()))) {
                Some(Some(ViewVal::Str(s))) => s.clone(),
                Some(Some(ViewVal::Num(n))) => format!("{}", n),
                _ => "—".into(),
            };
            ui.label(egui::RichText::new(txt).monospace());
        }
    }
}

/// 7-segment digit; `seg[0..7]` are a..g, `dp` the dot.
fn draw_seven_seg(p: &egui::Painter, rect: egui::Rect, color: egui::Color32, seg: &[bool], dp: bool) {
    let on = color;
    let off = dim(color);
    let w = 26.0;
    let h = 48.0;
    let x0 = rect.center().x - w / 2.0;
    let y0 = rect.top() + 6.0;
    let t = 4.0;
    let sc = |i: usize| if seg.get(i).copied().unwrap_or(false) { on } else { off };
    let hbar = |x: f32, y: f32, c: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(x + t, y - t / 2.0), egui::vec2(w - 2.0 * t, t)), 1.0, c);
    };
    let vbar = |x: f32, y: f32, c: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(x - t / 2.0, y + t), egui::vec2(t, h / 2.0 - 2.0 * t)), 1.0, c);
    };
    hbar(x0, y0, sc(0)); // a
    vbar(x0 + w, y0, sc(1)); // b
    vbar(x0 + w, y0 + h / 2.0, sc(2)); // c
    hbar(x0, y0 + h, sc(3)); // d
    vbar(x0, y0 + h / 2.0, sc(4)); // e
    vbar(x0, y0, sc(5)); // f
    hbar(x0, y0 + h / 2.0, sc(6)); // g
    p.circle_filled(egui::pos2(x0 + w + 6.0, y0 + h), 2.5, if dp { on } else { off });
}

/// Framebuffers of attached display devices.
fn display_panel(app: &mut IkIdeApp, ui: &mut egui::Ui) {
    // Names of placed display devices not yet rendered (before Run).
    let pending: Vec<String> = if app.bb_displays.is_empty() {
        app.bb_instances
            .iter()
            .filter_map(|inst| {
                app.device_catalog
                    .iter()
                    .find(|s| s.id == inst.spec_id && s.has_display)
                    .map(|s| s.name.clone())
            })
            .collect()
    } else {
        Vec::new()
    };
    if app.bb_displays.is_empty() && pending.is_empty() {
        return;
    }

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
            let tex = ui.ctx().load_texture(format!("bbdisp_{}", info.name), img, egui::TextureOptions::NEAREST);
            app.bb_textures.insert(info.name.clone(), (generation, tex));
        }

        if let Some((_, tex)) = app.bb_textures.get(&info.name) {
            ui.label(egui::RichText::new(format!("{}  ({}×{})", info.name, w, h)).weak().small());
            let scale = (240.0 / w as f32).min(2.0);
            let size = egui::vec2(w as f32 * scale, h as f32 * scale);
            let resp = ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), size)));
            ui.painter()
                .rect_stroke(resp.rect, egui::Rounding::ZERO, egui::Stroke::new(1.0, egui::Color32::from_gray(90)));
        }
    }
    ui.add_space(6.0);
}

// ---------------------------------------------------------------------------
// Bus monitor tabs
// ---------------------------------------------------------------------------

/// Devices placed on `bus`, listed read-only (managed on the Board tab).
fn bus_device_list(app: &IkIdeApp, ui: &mut egui::Ui, bus: Bus) {
    let names: Vec<String> = app
        .bb_instances
        .iter()
        .filter_map(|inst| {
            app.device_catalog
                .iter()
                .find(|s| s.id == inst.spec_id && s.bus == bus)
                .map(|s| {
                    let addr = s.address.map(|a| format!(" @0x{:02X}", a)).unwrap_or_default();
                    format!("{}{}", s.name, addr)
                })
        })
        .collect();
    if !names.is_empty() {
        ui.label(egui::RichText::new(format!("Devices: {}", names.join(", "))).weak().small());
        ui.separator();
    }
}

fn uart_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    if board::periph_addrs(app.breadboard.device_name()).uart.is_none() {
        unavailable(ui, "USART isn't modeled for this target in the simulator.");
        return;
    }
    bus_device_list(app, ui, Bus::Uart);

    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.bb_uart_plot, false, "Console");
        ui.selectable_value(&mut app.bb_uart_plot, true, "Plotter");
    });
    ui.separator();

    if app.bb_uart_plot {
        crate::ui::serial::render_plotter(app, ui);
        return;
    }

    ui.label(egui::RichText::new("Text the program transmits (TX).").weak().small());
    log_view(ui, &app.uart_log, "uart_log");

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let send = ui.add(
            egui::TextEdit::singleline(&mut app.uart_send)
                .desired_width(200.0)
                .hint_text("type to send to the program…"),
        );
        let enter = send.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui.add_enabled(app.live.is_some(), egui::Button::new("Send")).clicked();
        // Line ending, same options as the Serial Monitor.
        use crate::core::serial::LineEnding;
        egui::ComboBox::from_id_salt("bb_uart_ending")
            .selected_text(app.bb_uart_ending.label())
            .show_ui(ui, |ui| {
                for e in [LineEnding::None, LineEnding::Lf, LineEnding::Cr, LineEnding::CrLf] {
                    ui.selectable_value(&mut app.bb_uart_ending, e, e.label());
                }
            });
        if (enter || clicked) && !app.uart_send.is_empty() {
            let line = format!("{}{}", app.uart_send, app.bb_uart_ending.suffix());
            act.uart_send = Some(line.into_bytes());
            app.uart_send.clear();
        }
    });
}

fn spi_tab(app: &mut IkIdeApp, ui: &mut egui::Ui, act: &mut Actions) {
    if board::periph_addrs(app.breadboard.device_name()).spi.is_none() {
        unavailable(ui, "This target has no SPI.");
        return;
    }
    bus_device_list(app, ui, Bus::Spi);
    ui.label(
        egui::RichText::new("Full-duplex exchanges, in hex: MOSI→MISO (what the master sent → what it read back).")
            .weak()
            .small(),
    );
    log_view(ui, &app.spi_log, "spi_log");

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("MISO response:");
        let mut v = app.bb_spi_miso;
        if ui
            .add(egui::DragValue::new(&mut v).hexadecimal(2, false, true).prefix("0x"))
            .on_hover_text("Fallback byte returned to the master when no SPI device is attached.")
            .changed()
        {
            app.bb_spi_miso = v;
            act.spi_miso = Some(v);
        }
    });
}

fn i2c_tab(app: &mut IkIdeApp, ui: &mut egui::Ui) {
    bus_device_list(app, ui, Bus::I2c);
    ui.label(
        egui::RichText::new("TWI bus: [S] start, addr 0xNN R/W, data (hex; ← = read from the device), [P] stop.")
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

/// Scrolling monospace transcript pinned to the bottom.
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

// ---------------------------------------------------------------------------
// Action application
// ---------------------------------------------------------------------------

const SCRIPT_TEMPLATE: &str = r#"// My device — rename, wire and extend. Remove what you don't need.
//
// pins:  watch = observe an MCU output | drive = drive an MCU input | adc
// view:  led | rgbled | ledbar | sevenseg | button | slider | text
// bus:   none | uart | spi | i2c (+ address)
// State lives in `this` between calls; `this.fb` is the display, if declared.
// Full authoring guide with worked examples: assets/devices/README.md in the
// IKIDE repository (the built-in devices are all written this way).
fn meta() {
    #{
        name: "My Device",
        bus: "none",
        pins: #{ out: #{} },
        view: [#{ kind: "led", pin: "out", color: 0x52A0FF }],
    }
}

// fn init() { #{ count: 0 } }
// fn pin_set(name, level) { ... }
// fn on_view(id, value) { ... }
// fn i2c_address(addr, read) { addr == 0x42 }
// fn i2c_write(byte) { true }
// fn i2c_read(last) { 0xFF }
// fn spi_transfer(mosi) { 0xFF }
// fn uart_tx(byte) { ... }
// fn uart_poll() { () }
"#;

fn apply_actions(app: &mut IkIdeApp, act: Actions) {
    if let Some(idx) = act.remove {
        if idx < app.bb_instances.len() {
            app.bb_instances.remove(idx);
        }
    }
    if let Some(id) = act.add {
        let board_pins = app.breadboard.pins().to_vec();
        if let Some(spec) = app.device_catalog.iter().find(|s| s.id == id) {
            app.bb_instances.push(Instance::new(spec, &board_pins));
        }
    }
    if act.refresh_catalog {
        app.device_catalog = crate::core::devices::catalog();
    }
    if act.new_script {
        let dir = std::path::Path::new("devices");
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("my_device.rhai");
        if !path.exists() {
            let _ = std::fs::write(&path, SCRIPT_TEMPLATE);
        }
        if let Ok(abs) = path.canonicalize() {
            app.load_file(abs);
        }
        app.device_catalog = crate::core::devices::catalog();
        app.refresh_files();
    }
    if let Some(live) = &app.live {
        for (addr, bit, high) in act.inputs {
            live.set_input(addr, bit, high);
        }
        for (ch, val) in act.adc {
            live.set_adc(ch, val);
        }
        for (dev, id, value) in act.view_events {
            live.view_event(dev, &id, value);
        }
        if let Some(bytes) = act.uart_send {
            live.uart_send(bytes);
        }
        if let Some(b) = act.spi_miso {
            live.set_spi_miso(b);
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
