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

// The breadboard: a visual schematic wired to the live simulator.
//
// You add components (LEDs, push buttons), wire each to one of the target
// chip's GPIO pins, then press Run. The component states are driven straight
// from the running ik8bvm core: an LED lights when its pin's PORT bit is high
// *and* the pin is configured as an output (DDR bit set); a button, while held,
// drives its pin low (the classic active-low, internal-pull-up wiring).

use eframe::egui;

use crate::app::IkIdeApp;
use crate::core::board::{self, Pin};

/// A component kind the breadboard can place.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompKind {
    Led,
    Button,
}

/// One placed component, wired to at most one MCU pin.
pub struct Component {
    pub kind: CompKind,
    /// Index into the device's pin list (`board::pins`), if wired.
    pub pin: Option<usize>,
    /// LED colour choice (index into `LED_COLORS`).
    pub color_idx: usize,
    /// Button momentary state (true while held this frame).
    pub pressed: bool,
}

/// The breadboard model, owned by the app and persisted in memory for the
/// session. Pins are cached per target so we don't re-parse `gpio.ik` per frame.
#[derive(Default)]
pub struct Breadboard {
    pub components: Vec<Component>,
    cached_device: String,
    cached_pins: Vec<Pin>,
}

impl Breadboard {
    /// Refresh the cached pin list when the active target changes.
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

/// LED colour palette: lit colour for each choice. The unlit colour is derived
/// by dimming.
const LED_COLORS: &[(&str, egui::Color32)] = &[
    ("Red", egui::Color32::from_rgb(255, 70, 70)),
    ("Green", egui::Color32::from_rgb(80, 255, 110)),
    ("Blue", egui::Color32::from_rgb(90, 150, 255)),
    ("Yellow", egui::Color32::from_rgb(255, 220, 70)),
    ("White", egui::Color32::from_rgb(245, 245, 245)),
];

fn dim(c: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgb(
        (c.r() as f32 * 0.20) as u8 + 12,
        (c.g() as f32 * 0.20) as u8 + 12,
        (c.b() as f32 * 0.20) as u8 + 12,
    )
}

const ROW_H: f32 = 66.0;
const CHIP_W: f32 = 150.0;
const NODE_X: f32 = 190.0;
const NODE_W: f32 = 330.0;

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    if !app.show_breadboard {
        return;
    }

    // Resolve the active target from the focused buffer.
    let device = app
        .active_tab
        .and_then(|i| app.open_tabs.get(i))
        .and_then(|t| board::target_of(&t.content));

    let mut close = false;
    let mut start = false;
    let mut stop = false;
    let mut add_led = false;
    let mut add_button = false;
    let mut remove: Option<usize> = None;
    // Input commands to dispatch to the live engine after the UI pass, to keep
    // the breadboard model and the live handle borrowed disjointly.
    let mut inputs: Vec<(u32, u8, bool)> = Vec::new();

    egui::Window::new("breadboard")
        .title_bar(false)
        .resizable(true)
        .default_width(720.0)
        .default_height(440.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔌 Breadboard").strong());
                if let Some(dev) = &device {
                    ui.label(egui::RichText::new(format!("target: {}", dev)).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        close = true;
                    }
                });
            });
            ui.separator();

            let live = app.live.is_some();

            // --- Toolbar ---
            ui.horizontal_wrapped(|ui| {
                if !live {
                    if ui
                        .add_enabled(device.is_some(), egui::Button::new("▶ Run on Breadboard"))
                        .on_hover_text("Compile the active file and run it live; component states track the simulated I/O ports.")
                        .clicked()
                    {
                        start = true;
                    }
                } else {
                    if ui.button("⏹ Stop").clicked() {
                        stop = true;
                    }
                    let status = if app.live_running { "running" } else { "halted" };
                    ui.label(egui::RichText::new(format!("● {}  ·  {} cycles", status, app.live_cycles))
                        .color(if app.live_running { egui::Color32::from_rgb(80, 220, 110) } else { egui::Color32::GRAY }));
                }

                ui.separator();
                if ui.button("➕ LED").clicked() {
                    add_led = true;
                }
                if ui.button("➕ Button").clicked() {
                    add_button = true;
                }

                ui.separator();
                ui.add_enabled_ui(!live, |ui| {
                    let mut mhz = app.bb_clock_hz as f32 / 1_000_000.0;
                    ui.label("Clock:");
                    if ui
                        .add(egui::DragValue::new(&mut mhz).speed(0.5).range(0.0..=32.0).suffix(" MHz"))
                        .on_hover_text("Assumed CPU clock used to pace the run to real time. Match your code's F_CPU; 0 = run as fast as possible.")
                        .changed()
                    {
                        app.bb_clock_hz = (mhz * 1_000_000.0) as u32;
                    }
                });
            });

            if !app.live_status.is_empty() {
                ui.label(egui::RichText::new(&app.live_status).weak().small());
            }
            ui.separator();

            if device.is_none() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Open an .ik file with a `target` declaration to wire up its pins.").weak());
                });
                return;
            }
            let device = device.clone().unwrap();
            app.breadboard.ensure_pins(&device);
            let pins: Vec<Pin> = app.breadboard.pins().to_vec();

            if app.breadboard.components.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Add an LED or Button, then wire it to a pin.").weak());
                });
                return;
            }

            let n = app.breadboard.components.len();
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_size();
                let canvas_h = (n as f32 * ROW_H + 24.0).max(avail.y.max(120.0));
                let (resp, painter) = ui.allocate_painter(
                    egui::vec2(avail.x.max(NODE_X + NODE_W + 30.0), canvas_h),
                    egui::Sense::hover(),
                );
                let origin = resp.rect.min;

                // --- The MCU chip body ---
                let chip_rect = egui::Rect::from_min_size(
                    origin + egui::vec2(16.0, 8.0),
                    egui::vec2(CHIP_W, canvas_h - 16.0),
                );
                painter.rect_filled(chip_rect, egui::Rounding::same(8.0), egui::Color32::from_rgb(28, 30, 36));
                painter.rect_stroke(chip_rect, egui::Rounding::same(8.0), egui::Stroke::new(1.5, egui::Color32::from_rgb(70, 74, 84)));
                painter.text(
                    chip_rect.center_top() + egui::vec2(0.0, 14.0),
                    egui::Align2::CENTER_CENTER,
                    &device,
                    egui::FontId::monospace(13.0),
                    egui::Color32::from_rgb(170, 175, 185),
                );

                // --- One row per component: pad + wire (painter) then widgets ---
                for i in 0..n {
                    let row_top = origin + egui::vec2(NODE_X, 12.0 + i as f32 * ROW_H);
                    let node_rect = egui::Rect::from_min_size(row_top, egui::vec2(NODE_W, ROW_H - 12.0));
                    let center_y = node_rect.center().y;

                    // Wire + pad to the chip, when this component is wired.
                    let pin_idx = app.breadboard.components[i].pin;
                    if let Some(pidx) = pin_idx {
                        if let Some(pin) = pins.get(pidx) {
                            let pad = egui::pos2(chip_rect.right(), center_y);
                            // pad on the chip edge
                            painter.circle_filled(pad, 4.5, egui::Color32::from_rgb(200, 170, 90));
                            painter.text(
                                pad + egui::vec2(-8.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                &pin.name,
                                egui::FontId::monospace(11.0),
                                egui::Color32::from_rgb(170, 175, 185),
                            );
                            // wire from pad to the node's left edge
                            painter.line_segment(
                                [pad, egui::pos2(node_rect.left(), center_y)],
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(120, 124, 134)),
                            );
                        }
                    }

                    // Node background.
                    painter.rect_filled(node_rect, egui::Rounding::same(6.0), egui::Color32::from_rgb(38, 40, 47));

                    // Compute the live electrical state for this component.
                    let comp_kind = app.breadboard.components[i].kind;
                    let color = LED_COLORS[app.breadboard.components[i].color_idx.min(LED_COLORS.len() - 1)].1;
                    let led_on = matches!(comp_kind, CompKind::Led)
                        && app.live.is_some()
                        && pin_idx
                            .and_then(|p| pins.get(p))
                            .map(|pin| {
                                let port = *app.live_regs.get(&pin.port_addr).unwrap_or(&0);
                                let ddr = *app.live_regs.get(&pin.ddr_addr).unwrap_or(&0);
                                (port >> pin.bit) & 1 == 1 && (ddr >> pin.bit) & 1 == 1
                            })
                            .unwrap_or(false);

                    // Interactive widgets on top of the node.
                    let mut pressed_now = false;
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(node_rect.shrink(8.0)), |ui| {
                        ui.horizontal_centered(|ui| {
                            // Component glyph / state.
                            match comp_kind {
                                CompKind::Led => {
                                    let c = if led_on { color } else { dim(color) };
                                    ui.label(egui::RichText::new("⬤").size(26.0).color(c));
                                }
                                CompKind::Button => {
                                    let btn = ui.add(
                                        egui::Button::new(egui::RichText::new("⏺ hold").size(14.0))
                                            .min_size(egui::vec2(56.0, 30.0)),
                                    );
                                    pressed_now = btn.is_pointer_button_down_on();
                                }
                            }

                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                // Pin selector.
                                let cur = app.breadboard.components[i]
                                    .pin
                                    .and_then(|p| pins.get(p))
                                    .map(|p| p.name.clone())
                                    .unwrap_or_else(|| "— pin —".to_string());
                                egui::ComboBox::from_id_salt(("bb_pin", i))
                                    .selected_text(cur)
                                    .width(90.0)
                                    .show_ui(ui, |ui| {
                                        let mut sel = app.breadboard.components[i].pin;
                                        ui.selectable_value(&mut sel, None, "— none —");
                                        for (pi, p) in pins.iter().enumerate() {
                                            ui.selectable_value(&mut sel, Some(pi), &p.name);
                                        }
                                        app.breadboard.components[i].pin = sel;
                                    });

                                // LED colour selector.
                                if matches!(comp_kind, CompKind::Led) {
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
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("✖").on_hover_text("Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });

                    // Record button state and queue an input command when live.
                    app.breadboard.components[i].pressed = pressed_now;
                    if matches!(comp_kind, CompKind::Button) && app.live.is_some() {
                        if let Some(pin) = pin_idx.and_then(|p| pins.get(p)) {
                            // Active-low with internal pull-up: idle high, pressed low.
                            inputs.push((pin.pin_addr, pin.bit, !pressed_now));
                        }
                    }
                }
            });
        });

    // --- Apply collected actions (UI borrow of the model has ended) ---
    if let Some(idx) = remove {
        if idx < app.breadboard.components.len() {
            app.breadboard.components.remove(idx);
        }
    }
    if add_led {
        app.breadboard.components.push(Component {
            kind: CompKind::Led,
            pin: None,
            color_idx: 0,
            pressed: false,
        });
    }
    if add_button {
        app.breadboard.components.push(Component {
            kind: CompKind::Button,
            pin: None,
            color_idx: 0,
            pressed: false,
        });
    }
    if let Some(live) = &app.live {
        for (addr, bit, high) in inputs {
            live.set_input(addr, bit, high);
        }
    }
    if start {
        app.start_breadboard();
    }
    if stop {
        app.stop_breadboard();
    }
    if close {
        app.show_breadboard = false;
    }
}
