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

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    egui::SidePanel::right("right_panel")
        .resizable(true)
        .default_width(app.right_panel_width)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Simulation").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        app.show_vm_trace = false;
                    }
                    if ui.button("🗑 Clear").clicked() {
                        app.vm_output.clear();
                        app.vm_result = None;
                    }
                    // Copy the full simulation log (handy for long traces).
                    ui.add_enabled_ui(!app.vm_output.is_empty(), |ui| {
                        if ui.button("📋 Copy").on_hover_text("Copy the simulation log to the clipboard").clicked() {
                            ui.ctx().copy_text(app.vm_output.clone());
                        }
                    });
                });
            });
            ui.separator();

            // Structured end-of-run state, when a simulation has completed.
            if let Some(res) = &app.vm_result {
                render_state(ui, res);
                ui.separator();
            }

            ui.label(egui::RichText::new("Log").weak().small());
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut app.vm_output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .lock_focus(true)
                    );
                });
        });
}

/// Compact, readable view of the final core state from a simulation run.
fn render_state(ui: &mut egui::Ui, res: &crate::core::runner::VmResult) {
    egui::CollapsingHeader::new("Core State")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(format!("{} ({})", res.device, res.core));
            ui.label(egui::RichText::new(&res.halt_reason).italics());
            ui.add_space(4.0);

            egui::Grid::new("vm_state_grid").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                let mono = |s: String| egui::RichText::new(s).monospace();
                ui.label("Executed");
                ui.label(mono(format!("{}", res.executed)));
                ui.end_row();
                ui.label("Cycles");
                ui.label(mono(format!("{}", res.cycles)));
                ui.end_row();
                ui.label("PC");
                ui.label(mono(format!("0x{:06X}", res.pc)));
                ui.end_row();
                ui.label("SP");
                ui.label(mono(format!("0x{:04X}", res.sp)));
                ui.end_row();
                ui.label("SREG");
                ui.label(mono(format!("0x{:02X}", res.sreg)));
                ui.end_row();
            });

            ui.add_space(2.0);
            ui.label("Flags");
            ui.horizontal(|ui| {
                const NAMES: [&str; 8] = ["I", "T", "H", "S", "V", "N", "Z", "C"];
                for (i, name) in NAMES.iter().enumerate() {
                    let set = res.sreg & (0x80 >> i) != 0;
                    let txt = egui::RichText::new(*name).monospace();
                    let txt = if set { txt.strong().color(egui::Color32::from_rgb(0x4c, 0xc9, 0x4c)) } else { txt.weak() };
                    ui.label(txt);
                }
            });
        });

    egui::CollapsingHeader::new("Registers")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("vm_regs_grid").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
                for i in 0..32 {
                    ui.label(egui::RichText::new(format!("R{:<2}=0x{:02X}", i, res.regs[i])).monospace());
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });

    egui::CollapsingHeader::new("Memory")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("vm_mem_grid").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                let kb = |b: u32| format!("{} B ({:.1} KB)", b, b as f32 / 1024.0);
                ui.label("Flash");
                ui.label(egui::RichText::new(kb(res.flash_bytes)).monospace());
                ui.end_row();
                ui.label("SRAM");
                ui.label(egui::RichText::new(kb(res.sram_bytes)).monospace());
                ui.end_row();
                ui.label("EEPROM");
                ui.label(egui::RichText::new(kb(res.eeprom_bytes)).monospace());
                ui.end_row();
            });
        });
}
