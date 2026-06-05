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

            ui.label(egui::RichText::new("Explorer").strong());
            ui.separator();
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut to_load = None;
                    
                    let active_path = app.active_tab.map(|idx| app.open_tabs[idx].path.clone());
                    
                    if let Some(tree) = &app.workspace_tree {
                        if let Some(path) = crate::ui::explorer::render_node(
                            ui, 
                            tree, 
                            &active_path, 
                            app.workspace_dir.as_deref(), 
                            &mut app.action_popup
                        ) {
                            to_load = Some(path);
                        }
                    }
                    
                    if let Some(path) = to_load {
                        app.load_file(path);
                    }
                });
        });
}

fn render_node(
    ui: &mut egui::Ui, 
    node: &crate::core::workspace::FileNode, 
    selected: &Option<std::path::PathBuf>, 
    workspace_dir: Option<&std::path::Path>, 
    action_popup: &mut ActionPopup
) -> Option<std::path::PathBuf> {
    let mut clicked_path = None;
    
    if node.is_dir {
        let is_root = workspace_dir.map_or(false, |w| node.path == w);
        let header = egui::CollapsingHeader::new(format!("📁 {}", node.name))
            .default_open(is_root)
            .show(ui, |ui| {
                for child in &node.children {
                    if let Some(path) = render_node(ui, child, selected, workspace_dir, action_popup) {
                        clicked_path = Some(path);
                    }
                }
            });
            
        header.header_response.context_menu(|ui| {
            if ui.button("Create File").clicked() {
                *action_popup = ActionPopup::CreateFile {
                    parent_dir: node.path.clone(),
                    new_name: "new_file.ik".to_string(),
                };
                ui.close_menu();
            }
            if ui.button("Create Directory").clicked() {
                *action_popup = ActionPopup::CreateDir {
                    parent_dir: node.path.clone(),
                    new_name: "new_folder".to_string(),
                };
                ui.close_menu();
            }
            if !is_root {
                ui.separator();
                if ui.button("Rename").clicked() {
                    *action_popup = ActionPopup::Rename {
                        path: node.path.clone(),
                        new_name: node.name.clone(),
                    };
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    *action_popup = ActionPopup::Delete {
                        path: node.path.clone(),
                    };
                    ui.close_menu();
                }
            }
        });
        
    } else {
        let is_selected = selected.as_ref() == Some(&node.path);
        let icon = if node.name.ends_with(".hex") { "⚙" } else { "📄" };
        let response = ui.selectable_label(is_selected, format!("{} {}", icon, node.name));
        if response.clicked() {
            clicked_path = Some(node.path.clone());
        }
        
        response.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                *action_popup = ActionPopup::Rename {
                    path: node.path.clone(),
                    new_name: node.name.clone(),
                };
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                *action_popup = ActionPopup::Delete {
                    path: node.path.clone(),
                };
                ui.close_menu();
            }
        });
    }
    
    clicked_path
}
