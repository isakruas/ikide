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

use eframe::egui;
use crate::app::IkIdeApp;

/// Step-by-step debugger panel: load the active program into an in-process VM,
/// step it one instruction at a time, and show the live core state.
pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    let max_w = (ctx.screen_rect().width() * 0.5).clamp(280.0, 460.0);
    egui::SidePanel::right("debugger_panel")
        .resizable(true)
        .default_width(310.0)
        .min_width(260.0)
        .max_width(max_w)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Debugger").strong());
            });
            ui.separator();

            // Load / reset / step controls. start_debug compiles an .ik or parses
            // an open .hex; the rest act on the live VM.
            ui.horizontal(|ui| {
                if ui
                    .button("Load")
                    .on_hover_text("Compile/parse the active file into the debugger")
                    .clicked()
                {
                    if let Err(e) = app.start_debug() {
                        app.debug_status = e;
                    }
                }
                let has_vm = app.debug_vm.is_some();
                if ui.add_enabled(has_vm, egui::Button::new("Reset")).clicked() {
                    app.debug_reset();
                }
            });

            let runnable = app
                .debug_vm
                .as_ref()
                .map_or(false, |v| v.running && !v.unknown_opcode);
            ui.add_enabled_ui(runnable, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Step").on_hover_text("One instruction").clicked() {
                        app.debug_step(1);
                    }
                    if ui.button("×10").clicked() {
                        app.debug_step(10);
                    }
                    if ui.button("×100").clicked() {
                        app.debug_step(100);
                    }
                    if ui.button("Run").on_hover_text("Run until halt (capped)").clicked() {
                        app.debug_step(2_000_000);
                    }
                });
            });

            // Target for a raw .hex (an .ik carries its own target).
            ui.horizontal(|ui| {
                ui.label("Target:");
                let items: Vec<(String, String)> =
                    app.devices.iter().map(|(d, _)| (d.clone(), d.clone())).collect();
                let sel = app.debug_device.clone();
                if let Some(d) = crate::ui::widgets::filter_combo(ui, "dbg_target", &sel, &items) {
                    app.debug_device = d;
                    app.debug_reset();
                }
            });
            ui.separator();

            match &app.debug_vm {
                None => {
                    ui.label(
                        egui::RichText::new("No program loaded. Open an .ik or .hex and click Load.")
                            .weak(),
                    );
                }
                Some(vm) => {
                    render_state(ui, vm, app.debug_executed, &app.debug_status, &app.debug_ram_history)
                }
            }
        });
}

fn render_state(
    ui: &mut egui::Ui,
    vm: &ik8bvm::core::AvrVm,
    executed: u64,
    status: &str,
    ram_history: &[(u64, u16)],
) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Next instruction to execute, disassembled.
        let op = vm.flash_word(vm.pc);
        let op2 = vm.flash_word(vm.pc.wrapping_add(2));
        let (instr, _, _) = ik8bvm::disasm::disasm_static(op, op2, vm.pc);
        ui.label(
            egui::RichText::new(format!("→ {:06X}:  {}", vm.pc, instr))
                .monospace()
                .color(egui::Color32::from_rgb(120, 200, 255)),
        );
        let status_color = match status {
            "halted" => egui::Color32::from_gray(150),
            "unknown opcode" => egui::Color32::from_rgb(230, 130, 130),
            _ => egui::Color32::from_rgb(140, 210, 140),
        };
        ui.label(egui::RichText::new(status).small().color(status_color));
        ui.add_space(4.0);

        egui::Grid::new("dbg_summary").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
            ui.monospace("Executed");
            ui.monospace(format!("{}", executed));
            ui.end_row();
            ui.monospace("Cycles");
            ui.monospace(format!("{}", vm.cycles));
            ui.end_row();
            ui.monospace("PC");
            ui.monospace(format!("0x{:06X}", vm.pc));
            ui.end_row();
            ui.monospace("SP");
            ui.monospace(format!("0x{:04X}", vm.sp));
            ui.end_row();
            ui.monospace("SREG");
            ui.monospace(format!("0x{:02X}", vm.sreg));
            ui.end_row();
        });

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            for (bit, ch) in [
                (0x80u8, 'I'), (0x40, 'T'), (0x20, 'H'), (0x10, 'S'),
                (0x08, 'V'), (0x04, 'N'), (0x02, 'Z'), (0x01, 'C'),
            ] {
                let on = vm.sreg & bit != 0;
                let color = if on {
                    egui::Color32::from_rgb(140, 220, 140)
                } else {
                    egui::Color32::from_gray(90)
                };
                ui.label(egui::RichText::new(ch).monospace().strong().color(color));
            }
        });

        ui.separator();
        egui::Grid::new("dbg_regs").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
            for row in 0..8 {
                for col in 0..4 {
                    let i = row * 4 + col;
                    ui.monospace(format!("R{:<2}=0x{:02X}", i, vm.r[i]));
                }
                ui.end_row();
            }
        });

        ui.separator();
        let top = vm.sram_start + vm.sram_bytes - 1;
        let stack_now = top.saturating_sub(vm.sp as u32);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Stack usage").strong().small());
            ui.label(
                egui::RichText::new(format!("{} B / {} B", stack_now, vm.sram_bytes))
                    .small()
                    .monospace(),
            );
        });
        draw_ram_chart(ui, ram_history, vm.sram_bytes as f32);

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "Flash {} B   SRAM {} B   EEPROM {} B",
                vm.flash_bytes, vm.sram_bytes, vm.eeprom_bytes
            ))
            .small()
            .weak(),
        );
    });
}

/// Area chart of stack-byte usage over a fixed cycle window. The x-axis spans
/// the last [`crate::app::DEBUG_RAM_WINDOW_CYCLES`] cycles, so the curve slides
/// at a constant scale rather than compressing as the run grows.
fn draw_ram_chart(ui: &mut egui::Ui, history: &[(u64, u16)], sram_bytes: f32) {
    let window = crate::app::DEBUG_RAM_WINDOW_CYCLES;
    let (rect, _) = ui.allocate_at_least(
        egui::vec2(ui.available_width().max(150.0), 70.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let border = ui.visuals().widgets.noninteractive.bg_stroke.color.linear_multiply(0.6);
    let dim = ui.visuals().text_color().linear_multiply(0.5);
    painter.rect(rect, 4.0, egui::Color32::from_black_alpha(40), egui::Stroke::new(1.0, border));
    painter.line_segment(
        [egui::pos2(rect.left(), rect.center().y), egui::pos2(rect.right(), rect.center().y)],
        egui::Stroke::new(1.0, border.linear_multiply(0.4)),
    );

    let latest = history.last().map(|(c, _)| *c).unwrap_or(0);
    let min_c = latest.saturating_sub(window);
    let visible: Vec<&(u64, u16)> = history.iter().filter(|(c, _)| *c >= min_c).collect();

    let max_val = visible.iter().map(|(_, v)| *v).max().unwrap_or(0) as f32;
    let max_y = (max_val * 1.2).max(32.0).min(sram_bytes.max(32.0));
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("{:.0} B", max_y),
        egui::FontId::monospace(9.0),
        dim,
    );
    painter.text(
        egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{} cyc window", window),
        egui::FontId::monospace(9.0),
        dim,
    );

    if visible.len() >= 2 {
        let pts: Vec<egui::Pos2> = visible
            .iter()
            .map(|(c, v)| {
                let x = rect.left() + ((c - min_c) as f32 / window as f32) * rect.width();
                let y = rect.bottom() - ((*v as f32 / max_y) * rect.height()).min(rect.height());
                egui::pos2(x, y)
            })
            .collect();
        let fill = egui::Color32::from_rgba_unmultiplied(0, 180, 255, 18);
        for p in &pts {
            painter.line_segment([egui::pos2(p.x, rect.bottom()), *p], egui::Stroke::new(1.0, fill));
        }
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 180, 255))));
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "step to record usage",
            egui::FontId::proportional(11.0),
            ui.visuals().text_color().linear_multiply(0.4),
        );
    }
}
