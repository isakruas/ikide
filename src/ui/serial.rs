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
use crate::core::serial::{self, LineEnding, SerialConn, BAUD_RATES};

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    if !app.show_serial {
        return;
    }

    let mut close = false;
    egui::Window::new("serial_monitor")
        .title_bar(false)
        .resizable(true)
        .default_width(520.0)
        .default_height(360.0)
        .show(ctx, |ui| {
            // Header styled like the rest of the UI (normal-size strong label),
            // not egui's large window title.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔌 Serial Monitor").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").clicked() {
                        close = true;
                    }
                });
            });
            ui.separator();

            let connected = app.serial.is_some();

            // --- Connection controls ---
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!connected, |ui| {
                    // Editable port path (so custom devices can be typed) plus a
                    // dropdown of the ports the system reports.
                    ui.add(
                        egui::TextEdit::singleline(&mut app.serial_port)
                            .desired_width(140.0)
                            .hint_text("/dev/ttyUSB0"),
                    );
                    egui::ComboBox::from_id_salt("serial_port")
                        .selected_text("▾")
                        .width(16.0)
                        .show_ui(ui, |ui| {
                            // Enumerated only while the dropdown is open.
                            let ports = serial::list_ports();
                            if ports.is_empty() {
                                ui.label(egui::RichText::new("no ports found").weak());
                            }
                            for p in ports {
                                ui.selectable_value(&mut app.serial_port, p.clone(), p);
                            }
                        });

                    egui::ComboBox::from_id_salt("serial_baud")
                        .selected_text(format!("{} baud", app.serial_baud))
                        .show_ui(ui, |ui| {
                            for &b in BAUD_RATES {
                                ui.selectable_value(&mut app.serial_baud, b, format!("{}", b));
                            }
                        });
                });

                if connected {
                    if ui.button("⏹ Disconnect").clicked() {
                        app.serial = None; // Drop stops the reader thread.
                        app.serial_output.push_str(&format!("{} --- disconnected ---\n", crate::app::now_ts()));
                    }
                    ui.label(egui::RichText::new("● connected").color(egui::Color32::from_rgb(80, 200, 80)));
                } else {
                    let can_connect = !app.serial_port.is_empty();
                    if ui.add_enabled(can_connect, egui::Button::new("▶ Connect")).clicked() {
                        match SerialConn::open(&app.serial_port, app.serial_baud) {
                            Ok(conn) => {
                                app.serial = Some(conn);
                                app.serial_output.push_str(&format!(
                                    "{} --- connected to {} @ {} baud ---\n",
                                    crate::app::now_ts(), app.serial_port, app.serial_baud
                                ));
                                app.persist(); // remember port/baud
                            }
                            Err(e) => {
                                app.serial_output.push_str(&format!("\n[serial error] {}\n", e));
                            }
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🗑 Clear").clicked() {
                        app.serial_output.clear();
                    }
                });
            });

            ui.separator();

            // --- Received data ---
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut app.serial_output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .lock_focus(false),
                    );
                });

            ui.separator();

            // --- Send line ---
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("serial_ending")
                    .selected_text(app.serial_ending.label())
                    .show_ui(ui, |ui| {
                        for e in [LineEnding::None, LineEnding::Lf, LineEnding::Cr, LineEnding::CrLf] {
                            ui.selectable_value(&mut app.serial_ending, e, e.label());
                        }
                    });

                let mut send_now = false;
                ui.add_enabled_ui(connected, |ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.serial_input)
                            .desired_width(ui.available_width() - 60.0)
                            .hint_text("Type a line and press Enter"),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        send_now = true;
                        resp.request_focus();
                    }
                    if ui.button("Send").clicked() {
                        send_now = true;
                    }
                });
                if send_now {
                    send_line(app);
                }
            });
        });

    // Honor the header's close button.
    if close {
        app.show_serial = false;
    }
}

/// Write the input line (plus the chosen line ending) to the port and echo it.
fn send_line(app: &mut IkIdeApp) {
    if app.serial_input.is_empty() {
        return;
    }
    let line = format!("{}{}", app.serial_input, app.serial_ending.suffix());
    let echo = app.serial_input.clone();
    let result = app.serial.as_mut().map(|conn| conn.send(line.as_bytes()));
    match result {
        Some(Ok(())) => app.serial_output.push_str(&format!("> {}\n", echo)),
        Some(Err(e)) => app.serial_output.push_str(&format!("\n[send error] {}\n", e)),
        None => {}
    }
    app.serial_input.clear();
}
