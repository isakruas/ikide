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
use crate::core::analysis::Severity;

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    let diags = app.all_diagnostics();
    egui::TopBottomPanel::bottom("terminal_panel")
        .resizable(true)
        .min_height(100.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Output").strong());
                let errors = diags.iter().filter(|d| d.severity == Severity::Error).count();
                let warns = diags.len() - errors;
                if errors > 0 {
                    ui.label(
                        egui::RichText::new(format!("  ⛔ {} error(s)", errors))
                            .color(egui::Color32::from_rgb(255, 120, 120)),
                    );
                }
                if warns > 0 {
                    ui.label(
                        egui::RichText::new(format!("  ⚠ {} warning(s)", warns))
                            .color(egui::Color32::from_rgb(230, 180, 80)),
                    );
                }
                if diags.is_empty() {
                    ui.label(egui::RichText::new("  ✔ no problems").color(egui::Color32::from_rgb(120, 200, 120)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        app.show_terminal = false;
                    }
                    if ui.button("Clear").clicked() {
                        app.terminal_output.clear();
                    }
                    // Copy the whole output/VM log to the clipboard — handy when it
                    // is long enough that selecting it by hand is awkward.
                    ui.add_enabled_ui(!app.terminal_output.is_empty(), |ui| {
                        if ui.button("📋 Copy").on_hover_text("Copy all output to the clipboard").clicked() {
                            ui.ctx().copy_text(app.terminal_output.clone());
                        }
                    });
                });
            });
            ui.separator();

            // Problems list: each live diagnostic from the compiler and lints.
            // Rows are clickable — clicking jumps the editor to the offending
            // file and line (resolved across the workspace for imported modules).
            if !diags.is_empty() {
                let ws = app.workspace_dir.clone();
                let mut goto: Option<(Option<std::path::PathBuf>, usize)> = None;
                egui::ScrollArea::vertical()
                    .id_salt("problems_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for d in &diags {
                            // Locate label: "file.ik:line" for another file, else
                            // "line N" for the active buffer.
                            let loc = match (&d.file, d.line) {
                                (Some(f), l) if l > 0 => {
                                    let shown = ws
                                        .as_ref()
                                        .and_then(|w| f.strip_prefix(w).ok())
                                        .unwrap_or(f.as_path())
                                        .to_string_lossy()
                                        .into_owned();
                                    format!("{}:{}: ", shown, l)
                                }
                                (None, l) if l > 0 => format!("line {}: ", l),
                                _ => String::new(),
                            };
                            let (icon, color) = match d.severity {
                                Severity::Error => ("⛔", egui::Color32::from_rgb(255, 140, 140)),
                                Severity::Warning => ("⚠", egui::Color32::from_rgb(230, 180, 80)),
                            };
                            let navigable = d.line > 0;
                            let resp = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{} {}{}", icon, loc, d.message))
                                        .monospace()
                                        .color(color),
                                )
                                .sense(egui::Sense::click()),
                            );
                            let resp = if navigable {
                                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(
                                        egui::RichText::new(match &d.raw {
                                            Some(raw) => format!("Click to go to the code\n\n{}", raw),
                                            None => "Click to go to the code".to_string(),
                                        })
                                        .monospace(),
                                    )
                            } else if let Some(raw) = &d.raw {
                                resp.on_hover_text(egui::RichText::new(raw).monospace())
                            } else {
                                resp
                            };
                            if navigable && resp.clicked() {
                                goto = Some((d.file.clone(), d.line));
                            }
                        }
                    });
                if let Some((file, line)) = goto {
                    app.goto_diagnostic(&file, line);
                    app.show_terminal = true;
                    ui.ctx().request_repaint(); // apply the scroll next frame
                }
                ui.separator();
            }

            egui::ScrollArea::vertical()
                .id_salt("output_scroll")
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut app.terminal_output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(8)
                            .lock_focus(true)
                    );
                });
        });
}
