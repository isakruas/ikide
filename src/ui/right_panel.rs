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
    // Cap panel width so neither one grows to swallow the window: at most 60% of
    // the screen, hard-capped at 640px, with a sane minimum.
    let max_w = (ctx.screen_rect().width() * 0.6).clamp(300.0, 640.0);

    // AI Chat is its own panel (rightmost) — independent from the simulation view.
    if app.show_ai_chat {
        egui::SidePanel::right("ai_chat_panel")
            .resizable(true)
            .default_width(380.0_f32.min(max_w))
            .min_width(280.0)
            .max_width(max_w)
            .show(ctx, |ui| {
                render_ai_chat(ui, app, ctx);
            });
    }

    // Simulation is its own panel, with its own header and controls.
    if app.show_vm_trace {
        egui::SidePanel::right("sim_panel")
            .resizable(true)
            .default_width(app.right_panel_width.min(max_w))
            .min_width(260.0)
            .max_width(max_w)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Simulation").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").on_hover_text("Close").clicked() {
                            app.show_vm_trace = false;
                        }
                        if ui.button("🗑 Clear").clicked() {
                            app.vm_output.clear();
                            app.vm_result = None;
                        }
                        ui.add_enabled_ui(!app.vm_output.is_empty(), |ui| {
                            if ui.button("📋 Copy").on_hover_text("Copy simulation log").clicked() {
                                ui.ctx().copy_text(app.vm_output.clone());
                            }
                        });
                    });
                });
                ui.separator();
                render_simulation_tab(ui, app);
            });
    }
}

fn render_simulation_tab(ui: &mut egui::Ui, app: &mut IkIdeApp) {
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
}

fn render_ai_chat(ui: &mut egui::Ui, app: &mut IkIdeApp, ctx: &egui::Context) {
    // Minimal header: title, clear, close. No agent/config bar — the agent is
    // configured under Preferences → AI Chat.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("AI Chat").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✖").on_hover_text("Close").clicked() {
                app.show_ai_chat = false;
            }
            if ui.button("🗑").on_hover_text("Clear conversation").clicked() {
                app.ai_chat_history.clear();
            }
            let eye = if app.agent_hide_system { "👁" } else { "🛠" };
            let hint = if app.agent_hide_system {
                "Show tool/system activity"
            } else {
                "Hide tool/system activity"
            };
            if ui.button(eye).on_hover_text(hint).clicked() {
                app.agent_hide_system = !app.agent_hide_system;
                app.persist();
            }
        });
    });
    ui.separator();

    // Conversation fills the panel; the input is pinned at the bottom.
    let input_h = 46.0;
    let history_height = (ui.available_height() - input_h).max(60.0);
    egui::ScrollArea::vertical()
        .max_height(history_height)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if app.ai_chat_history.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new("Ask the assistant to write, compile or simulate ik code.")
                            .weak(),
                    );
                });
            } else {
                for msg in &app.ai_chat_history {
                    // Optionally hide the agent's tool/system activity for a clean view.
                    if app.agent_hide_system && msg.role == "system" {
                        continue;
                    }
                    render_message(ui, msg);
                    ui.add_space(6.0);
                }
            }

            if app.ai_agent_running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(&app.ai_agent_status).italics().weak());
                });
            }
        });

    ui.separator();

    // Input row, pinned at the bottom.
    ui.horizontal(|ui| {
        if app.ai_agent_running {
            if ui.button("Stop").clicked() {
                app.agent_rx = None;
                app.agent_tx = None;
                app.ai_agent_running = false;
                app.ai_agent_status = "Idle".to_string();
                app.ai_chat_history.push(crate::core::agent::ChatMessage {
                    role: "system".to_string(),
                    content: "Agent stopped by user.".to_string(),
                });
            }
        }
        ui.add_enabled_ui(!app.ai_agent_running, |ui| {
            let input_resp = ui.add_sized(
                [ui.available_width() - 64.0, 30.0],
                egui::TextEdit::singleline(&mut app.ai_chat_input)
                    .hint_text("Message the assistant..."),
            );
            let enter_pressed =
                input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Send").clicked() || enter_pressed {
                send_ai_message(app, ctx);
            }
        });
    });
}

/// One chat bubble: role label above its content.
fn render_message(ui: &mut egui::Ui, msg: &crate::core::agent::ChatMessage) {
    let (label, color) = match msg.role.as_str() {
        "user" => ("You", egui::Color32::from_rgb(100, 160, 240)),
        "assistant" => ("Assistant", egui::Color32::from_rgb(120, 220, 120)),
        "system" => ("System", egui::Color32::from_rgb(180, 180, 100)),
        _ => ("Tool", egui::Color32::from_rgb(160, 100, 160)),
    };
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(label).strong().color(color).small());
        ui.label(egui::RichText::new(&msg.content));
    });
}

fn send_ai_message(app: &mut IkIdeApp, ctx: &egui::Context) {
    let msg = app.ai_chat_input.trim().to_string();
    if msg.is_empty() {
        return;
    }

    app.ai_chat_history.push(crate::core::agent::ChatMessage {
        role: "user".to_string(),
        content: msg.clone(),
    });
    app.ai_chat_input.clear();

    app.ai_agent_running = true;
    app.ai_agent_status = "Starting agent...".to_string();

    let (tx, rx) = std::sync::mpsc::channel();
    app.agent_rx = Some(rx);
    app.agent_tx = Some(tx.clone());

    let workspace_dir = app.workspace_dir.clone();
    let agent_kind = app.llm_provider.clone();
    let autonomy = app.agent_autonomy.clone();
    // The file currently open in the editor — the default target for ide_compile
    // / ide_simulate when the agent doesn't name one.
    let active_file = app
        .active_tab
        .and_then(|i| app.open_tabs.get(i))
        .map(|t| t.path.clone());
    let ctx_clone = ctx.clone();

    // Each turn is an independent, headless CLI run wired to ikmcp + the IDE bridge.
    std::thread::spawn(move || {
        crate::core::agent::run_cli_agent(workspace_dir, agent_kind, autonomy, msg, active_file, tx);
        // Request an immediate redraw on thread exit so the final reply shows.
        ctx_clone.request_repaint();
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
