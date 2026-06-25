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
use crate::app::{ActionPopup, IkIdeApp};

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    egui::SidePanel::left("left_panel")
        .resizable(true)
        .default_width(app.left_panel_width)
        .show(ctx, |ui| {
            if app.show_stats {
                egui::TopBottomPanel::bottom("left_bottom_stats")
                    .resizable(true)
                    .min_height(100.0)
                    .show_inside(ui, |ui| {
                        ui.label(egui::RichText::new("Resource Usage").strong());
                        ui.separator();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                        match &app.stats_data {
                            Ok(Some(stats)) => {
                                ui.label(egui::RichText::new(format!("Microcontroller: {} ({})", stats.target_name, stats.target_core))
                                    .small()
                                    .italics()
                                    .color(egui::Color32::from_rgb(180, 180, 180)));
                                ui.add_space(8.0);
                                
                                let mut draw_stat = |name: &str, used: u32, total: u32, pct: u32| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(name).small().strong());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{} / {} B", used, total)).small());
                                        });
                                    });
                                    
                                    let fraction = if total == 0 { 0.0 } else { used as f32 / total as f32 };
                                    
                                    // Custom colors based on usage
                                    let fill_color = if fraction > 0.9 {
                                        egui::Color32::from_rgb(220, 50, 50) // Red
                                    } else if fraction > 0.75 {
                                        egui::Color32::from_rgb(220, 180, 40) // Yellow/Orange
                                    } else {
                                        egui::Color32::from_rgb(60, 140, 220) // Blue
                                    };
                                    
                                    let bar = egui::ProgressBar::new(fraction)
                                        .fill(fill_color)
                                        .text(format!("{}%", pct));
                                        
                                    ui.add(bar);
                                    ui.add_space(6.0);
                                };

                                draw_stat("Program (Flash)", stats.prog_used, stats.prog_total, stats.prog_pct);
                                draw_stat("SRAM", stats.sram_used, stats.sram_total, stats.sram_pct);
                                draw_stat("EEPROM", stats.eeprom_used, stats.eeprom_total, stats.eeprom_pct);

                                // Registers are counts, not bytes, so render with their own unit.
                                if stats.regs_total > 0 {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Registers").small().strong());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let extra = if stats.spills > 0 {
                                                format!("   ({} spilled)", stats.spills)
                                            } else {
                                                String::new()
                                            };
                                            ui.label(egui::RichText::new(format!("{} / {}{}", stats.regs_used, stats.regs_total, extra)).small());
                                        });
                                    });
                                    let frac = stats.regs_used as f32 / stats.regs_total as f32;
                                    let fill = if frac > 0.9 {
                                        egui::Color32::from_rgb(220, 50, 50)
                                    } else if frac > 0.75 {
                                        egui::Color32::from_rgb(220, 180, 40)
                                    } else {
                                        egui::Color32::from_rgb(60, 140, 220)
                                    };
                                    ui.add(egui::ProgressBar::new(frac).fill(fill).text(format!("{}%", stats.regs_used * 100 / stats.regs_total)));
                                    ui.add_space(6.0);
                                }
                            }
                            Ok(None) => {
                                ui.label(egui::RichText::new("Save file to see stats...").italics().small());
                            }
                            Err(e) => {
                                if e.contains("top-level device target is required") {
                                    ui.label(egui::RichText::new("No target defined.\nAdd `target <name>` at the top of the file.").small().color(egui::Color32::YELLOW));
                                } else {
                                    ui.label(egui::RichText::new(e).color(egui::Color32::RED).small());
                                }
                            }
                        }
                    });
                });
            }

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Explorer").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔄").on_hover_text("Refresh").clicked() {
                        app.refresh_files();
                    }
                    if let Some(root) = app.workspace_dir.clone() {
                        if ui.button("📁+").on_hover_text("New Folder").clicked() {
                            app.action_popup = ActionPopup::CreateDir {
                                parent_dir: root.clone(),
                                new_name: "new_folder".to_string(),
                            };
                        }
                        if ui.button("📄+").on_hover_text("New File").clicked() {
                            app.action_popup = ActionPopup::CreateFile {
                                parent_dir: root,
                                new_name: "new_file.ik".to_string(),
                            };
                        }
                    }
                });
            });
            ui.separator();
            let mut to_load = None;
            let mut needs_refresh = false;
            // Screen rect of each file row this frame, for rubber-band hit-testing.
            let mut row_rects: Vec<(std::path::PathBuf, egui::Rect)> = Vec::new();
            let active_path = app.active_tab.map(|idx| app.open_tabs[idx].path.clone());

            let scroll_out = egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(tree) = &app.workspace_tree {
                        if let Some(path) = crate::ui::explorer::render_node(
                            ui,
                            tree,
                            &active_path,
                            &mut app.explorer_selection,
                            app.workspace_dir.as_deref(),
                            &mut app.action_popup,
                            &mut needs_refresh,
                            &mut row_rects,
                        ) {
                            to_load = Some(path);
                        }
                    }
                });

            // Rubber-band: drag across the file list to select every file the
            // rectangle touches. Uses the global pointer (not per-row hover, which
            // egui suppresses mid-drag). Ctrl/Cmd keeps the existing selection.
            let viewport = scroll_out.inner_rect;
            let pointer = ui.input(|i| i.pointer.clone());
            if pointer.primary_down() {
                if let (Some(origin), Some(cur)) = (pointer.press_origin(), pointer.interact_pos()) {
                    if viewport.contains(origin) && (origin - cur).length() > 4.0 {
                        let band = egui::Rect::from_two_pos(origin, cur);
                        let additive = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                        if !additive {
                            app.explorer_selection.clear();
                        }
                        for (path, rect) in &row_rects {
                            if band.intersects(*rect) {
                                app.explorer_selection.insert(path.clone());
                            }
                        }
                        ui.painter().rect_filled(
                            band,
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(100, 150, 255, 30),
                        );
                    }
                }
            }

            if let Some(path) = to_load {
                app.load_file(path);
            }
            if needs_refresh {
                app.refresh_files();
            }
        });
}

/// Pick a non-clashing "<name> copy" path next to `src` for Duplicate.
fn duplicate_target(src: &std::path::Path) -> std::path::PathBuf {
    let parent = src.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1.. {
        let suffix = if n == 1 { " copy".to_string() } else { format!(" copy {}", n) };
        let candidate = parent.join(format!("{}{}{}", stem, suffix, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn render_node(
    ui: &mut egui::Ui,
    node: &crate::core::workspace::FileNode,
    active: &Option<std::path::PathBuf>,
    selection: &mut std::collections::HashSet<std::path::PathBuf>,
    workspace_dir: Option<&std::path::Path>,
    action_popup: &mut ActionPopup,
    needs_refresh: &mut bool,
    row_rects: &mut Vec<(std::path::PathBuf, egui::Rect)>,
) -> Option<std::path::PathBuf> {
    let mut clicked_path = None;

    if node.is_dir {
        let is_root = workspace_dir.map_or(false, |w| node.path == w);
        let header = egui::CollapsingHeader::new(format!("📁 {}", node.name))
            .default_open(is_root)
            .show(ui, |ui| {
                for child in &node.children {
                    if let Some(path) = render_node(ui, child, active, selection, workspace_dir, action_popup, needs_refresh, row_rects) {
                        clicked_path = Some(path);
                    }
                }
            });

        header.header_response.context_menu(|ui| {
            if ui.button("📄 New File").clicked() {
                *action_popup = ActionPopup::CreateFile {
                    parent_dir: node.path.clone(),
                    new_name: "new_file.ik".to_string(),
                };
                ui.close_menu();
            }
            if ui.button("📁 New Folder").clicked() {
                *action_popup = ActionPopup::CreateDir {
                    parent_dir: node.path.clone(),
                    new_name: "new_folder".to_string(),
                };
                ui.close_menu();
            }
            ui.separator();
            if ui.button("📋 Copy Path").clicked() {
                ui.ctx().copy_text(node.path.to_string_lossy().into_owned());
                ui.close_menu();
            }
            if !is_root {
                if ui.button("✏ Rename").clicked() {
                    *action_popup = ActionPopup::Rename {
                        path: node.path.clone(),
                        new_name: node.name.clone(),
                    };
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    *action_popup = ActionPopup::Delete {
                        paths: vec![node.path.clone()],
                    };
                    ui.close_menu();
                }
            }
        });
    } else {
        let is_selected = selection.contains(&node.path) || active.as_ref() == Some(&node.path);
        let response = ui.selectable_label(is_selected, format!("📄 {}", node.name));
        row_rects.push((node.path.clone(), response.rect));
        if response.clicked() {
            // Ctrl/Cmd+click toggles this file in the multi-selection without
            // opening it; a plain click selects just this one and opens it.
            let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
            if multi {
                if !selection.remove(&node.path) {
                    selection.insert(node.path.clone());
                }
            } else {
                selection.clear();
                selection.insert(node.path.clone());
                clicked_path = Some(node.path.clone());
            }
        }

        response.context_menu(|ui| {
            if ui.button("📂 Open").clicked() {
                clicked_path = Some(node.path.clone());
                ui.close_menu();
            }
            ui.separator();
            if ui.button("✏ Rename").clicked() {
                *action_popup = ActionPopup::Rename {
                    path: node.path.clone(),
                    new_name: node.name.clone(),
                };
                ui.close_menu();
            }
            if ui.button("Duplicate").clicked() {
                let dst = duplicate_target(&node.path);
                if std::fs::copy(&node.path, &dst).is_ok() {
                    *needs_refresh = true;
                }
                ui.close_menu();
            }
            if ui.button("📋 Copy Path").clicked() {
                ui.ctx().copy_text(node.path.to_string_lossy().into_owned());
                ui.close_menu();
            }
            ui.separator();
            let multi = selection.len() > 1 && selection.contains(&node.path);
            let label = if multi {
                format!("Delete {} selected", selection.len())
            } else {
                "Delete".to_string()
            };
            if ui.button(label).clicked() {
                let paths = if multi {
                    selection.iter().cloned().collect()
                } else {
                    vec![node.path.clone()]
                };
                *action_popup = ActionPopup::Delete { paths };
                ui.close_menu();
            }
        });
    }

    clicked_path
}
