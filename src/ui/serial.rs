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
        .min_size([360.0, 220.0])
        .max_size([
            (ctx.screen_rect().width() * 0.9).max(520.0),
            (ctx.screen_rect().height() * 0.9).max(360.0),
        ])
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

            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.serial_tab, crate::app::SerialTab::Console, "Console");
                ui.selectable_value(&mut app.serial_tab, crate::app::SerialTab::Plotter, "Plotter");
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
                    if ui.button("◻ Disconnect").clicked() {
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
                    if ui.button("◻ Clear").clicked() {
                        app.serial_output.clear();
                    }
                });
            });

            ui.separator();

            if app.serial_tab == crate::app::SerialTab::Console {
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
            } else {
                render_plotter(app, ui);
            }
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

#[allow(non_snake_case)]
pub fn render_plotter(app: &mut IkIdeApp, ui: &mut egui::Ui) {
    let colors = [
        egui::Color32::from_rgb(255, 85, 85),    // Red
        egui::Color32::from_rgb(85, 255, 85),    // Green
        egui::Color32::from_rgb(85, 85, 255),    // Blue
        egui::Color32::from_rgb(255, 255, 85),   // Yellow
        egui::Color32::from_rgb(255, 85, 255),   // Magenta
        egui::Color32::from_rgb(85, 255, 255),   // Cyan
        egui::Color32::from_rgb(255, 170, 85),   // Orange
        egui::Color32::from_rgb(170, 85, 255),   // Purple
    ];

    let num_channels = app.plot_history.iter().map(|v| v.len()).max().unwrap_or(0);

    // Top control bar
    ui.horizontal(|ui| {
        if ui.button("◻ Clear Plot").clicked() {
            app.plot_history.clear();
            app.plot_labels.clear();
            app.plot_visible.clear();
        }
        ui.label(format!("Total points: {}", app.plot_history.len()));
        
        ui.separator();
        
        ui.menu_button("◻ Settings", |ui| {
            ui.checkbox(&mut app.plot_grid, "Grid View");
            ui.checkbox(&mut app.plot_center_line, "Center Line");
            ui.checkbox(&mut app.plot_show_stats, "Show Stats Table");
            ui.separator();
            ui.label("Window Size:");
            ui.add(egui::Slider::new(&mut app.plot_window_size, 100..=5000).text("points"));
        });
    });
    
    // Channel selection checkboxes
    ui.horizontal_wrapped(|ui| {
        ui.label("Visible Channels: ");
        for ch in 0..num_channels {
            if ch < app.plot_visible.len() {
                let channel_name = app.plot_labels.get(ch).cloned().unwrap_or_else(|| format!("Ch {}", ch + 1));
                let color = colors[ch % colors.len()];
                ui.checkbox(&mut app.plot_visible[ch], egui::RichText::new(channel_name).color(color));
            }
        }
    });
    
    ui.add_space(4.0);
    
    // Determine canvas height based on stats table visibility.
    // Calculate the height of the stats table dynamically to prevent infinite window growth.
    let mut stats_height = 10.0;
    if app.plot_show_stats {
        let mut visible_channels = 0;
        for ch in 0..num_channels {
            if ch < app.plot_visible.len() && app.plot_visible[ch] {
                visible_channels += 1;
            }
        }
        stats_height = 45.0 + (visible_channels as f32 * 20.0);
    }
    let canvas_height = ui.available_height() - stats_height;
    let canvas_height = canvas_height.max(150.0);
    
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), canvas_height), egui::Sense::hover());
    let rect = response.rect;
    let bg_color = egui::Color32::from_rgb(20, 20, 25);
    painter.rect_filled(rect, 4.0, bg_color);
    
    if app.plot_history.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No data received yet.\nSend numbers (e.g. \"sin:23.4, cos:60\\n\") to plot.",
            egui::FontId::proportional(14.0),
            egui::Color32::GRAY,
        );
        return;
    }
    
    if num_channels == 0 {
        return;
    }
    
    let points_len = app.plot_history.len();
    let W = app.plot_window_size;
    let start_idx = if points_len > W {
        points_len - W
    } else {
        0
    };
    let W_f = W as f32;
    
    // Find min and max values only among visible channels inside the current window
    let mut min_val = f32::INFINITY;
    let mut max_val = f32::NEG_INFINITY;
    let mut has_visible_data = false;
    for i in start_idx..points_len {
        let row = &app.plot_history[i];
        for ch in 0..num_channels {
            if ch < app.plot_visible.len() && !app.plot_visible[ch] {
                continue;
            }
            if let Some(&val) = row.get(ch) {
                if val.is_finite() {
                    has_visible_data = true;
                    if val < min_val { min_val = val; }
                    if val > max_val { max_val = val; }
                }
            }
        }
    }
    
    if !has_visible_data || min_val.is_infinite() || max_val.is_infinite() {
        min_val = 0.0;
        max_val = 100.0;
    } else if min_val == max_val {
        min_val -= 1.0;
        max_val += 1.0;
    } else {
        let diff = max_val - min_val;
        min_val -= diff * 0.05;
        max_val += diff * 0.05;
    }
    
    let grid_color = egui::Color32::from_rgb(40, 40, 50);
    let text_color = egui::Color32::from_rgb(150, 150, 160);
    let font_id = egui::FontId::monospace(10.0);
    
    // Draw Y axis horizontal grid lines and labels
    for i in 0..=4 {
        let pct = i as f32 / 4.0;
        let y = rect.bottom() - pct * rect.height();
        if app.plot_grid {
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0, grid_color),
            );
        }
        
        let val = min_val + pct * (max_val - min_val);
        painter.text(
            egui::pos2(rect.left() + 5.0, y - 2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{:.2}", val),
            font_id.clone(),
            text_color,
        );
    }
    
    // Draw X axis vertical grid lines and index labels
    for i in 0..=4 {
        let pct = i as f32 / 4.0;
        let x = rect.left() + pct * rect.width();
        if app.plot_grid {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0, grid_color),
            );
        }
        
        let idx = start_idx + (pct * W_f) as usize;
        painter.text(
            egui::pos2(x, rect.bottom() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{}", idx),
            font_id.clone(),
            text_color,
        );
    }
    
    // Draw center line if enabled
    if app.plot_center_line {
        let y = rect.bottom() - 0.5 * rect.height();
        let stroke = egui::Stroke::new(1.2, egui::Color32::from_rgba_unmultiplied(255, 165, 0, 150));
        let mut x = rect.left();
        while x < rect.right() {
            painter.line_segment(
                [egui::pos2(x, y), egui::pos2((x + 8.0).min(rect.right()), y)],
                stroke,
            );
            x += 16.0;
        }
        painter.text(
            egui::pos2(rect.right() - 5.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("Center: {:.2}", (min_val + max_val) / 2.0),
            font_id.clone(),
            egui::Color32::from_rgb(255, 170, 85),
        );
    }
    
    // Draw lines only for visible channels
    for ch in 0..num_channels {
        if ch < app.plot_visible.len() && !app.plot_visible[ch] {
            continue;
        }
        let mut points = Vec::new();
        for i in start_idx..points_len {
            if let Some(&val) = app.plot_history[i].get(ch) {
                if val.is_finite() {
                    let rel_idx = (i - start_idx) as f32;
                    let x = rect.left() + (rel_idx / W_f) * rect.width();
                    let y = rect.bottom() - ((val - min_val) / (max_val - min_val)) * rect.height();
                    points.push(egui::pos2(x, y));
                }
            }
        }
        if points.len() >= 2 {
            let stroke = egui::Stroke::new(1.5, colors[ch % colors.len()]);
            for window in points.windows(2) {
                painter.line_segment([window[0], window[1]], stroke);
            }
        }
    }
    
    // Draw vertical cursor and tooltip on hover
    if let Some(hover_pos) = response.hover_pos() {
        if rect.contains(hover_pos) {
            let pct = (hover_pos.x - rect.left()) / rect.width();
            let hover_offset = (pct * W_f) as usize;
            let hover_idx = start_idx + hover_offset;
            if hover_idx < points_len {
                // Draw vertical indicator line
                painter.line_segment(
                    [egui::pos2(hover_pos.x, rect.top()), egui::pos2(hover_pos.x, rect.bottom())],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(200, 200, 200, 70)),
                );
                
                // Draw rich tooltip with values
                response.show_tooltip_ui(|ui| {
                    ui.label(egui::RichText::new(format!("Datapoint #{}", hover_idx)).strong());
                    ui.separator();
                    for ch in 0..num_channels {
                        if ch < app.plot_visible.len() && app.plot_visible[ch] {
                            let val_str = if let Some(row) = app.plot_history.get(hover_idx) {
                                if let Some(&val) = row.get(ch) {
                                    format!("{:.2}", val)
                                } else {
                                    "N/A".to_string()
                                }
                            } else {
                                "N/A".to_string()
                            };
                            let channel_name = app.plot_labels.get(ch).cloned().unwrap_or_else(|| format!("Ch {}", ch + 1));
                            ui.colored_label(colors[ch % colors.len()], format!("{}: {}", channel_name, val_str));
                        }
                    }
                });
            }
        }
    }
    
    // Legend (top right, lists only visible channels with their current values)
    let mut legend_pos = egui::pos2(rect.right() - 10.0, rect.top() + 10.0);
    for ch in 0..num_channels {
        if ch < app.plot_visible.len() && !app.plot_visible[ch] {
            continue;
        }
        let color = colors[ch % colors.len()];
        let last_val_str = if let Some(row) = app.plot_history.last() {
            if let Some(&val) = row.get(ch) {
                format!("{:.2}", val)
            } else {
                "N/A".to_string()
            }
        } else {
            "N/A".to_string()
        };
        
        let channel_name = app.plot_labels.get(ch).cloned().unwrap_or_else(|| format!("Ch {}", ch + 1));
        let label = format!("{}: {}", channel_name, last_val_str);
        painter.rect_filled(
            egui::Rect::from_min_size(legend_pos - egui::vec2(120.0, 0.0), egui::vec2(8.0, 8.0)),
            1.0,
            color,
        );
        painter.text(
            legend_pos - egui::vec2(108.0, -1.0),
            egui::Align2::LEFT_CENTER,
            label,
            font_id.clone(),
            text_color,
        );
        legend_pos.y += 14.0;
    }
    
    // Render Stats Table if enabled
    if app.plot_show_stats {
        ui.add_space(8.0);
        
        // Header Row
        ui.columns(5, |columns| {
            columns[0].label(egui::RichText::new("Channel").strong());
            columns[1].label(egui::RichText::new("Min").strong());
            columns[2].label(egui::RichText::new("Max").strong());
            columns[3].label(egui::RichText::new("Average").strong());
            columns[4].label(egui::RichText::new("Last").strong());
        });
        ui.separator();
        
        for ch in 0..num_channels {
            if ch < app.plot_visible.len() && !app.plot_visible[ch] {
                continue;
            }
            let color = colors[ch % colors.len()];
            let channel_name = app.plot_labels.get(ch).cloned().unwrap_or_else(|| format!("Ch {}", ch + 1));
            
            let mut c_min = f32::INFINITY;
            let mut c_max = f32::NEG_INFINITY;
            let mut c_sum = 0.0;
            let mut c_count = 0;
            let mut last_val = None;
            
            for i in start_idx..points_len {
                if let Some(&val) = app.plot_history[i].get(ch) {
                    if val.is_finite() {
                        if val < c_min { c_min = val; }
                        if val > c_max { c_max = val; }
                        c_sum += val;
                        c_count += 1;
                        last_val = Some(val);
                    }
                }
            }
            
            ui.columns(5, |columns| {
                columns[0].colored_label(color, channel_name);
                if c_count > 0 {
                    columns[1].label(format!("{:.2}", c_min));
                    columns[2].label(format!("{:.2}", c_max));
                    columns[3].label(format!("{:.2}", c_sum / c_count as f32));
                    columns[4].label(format!("{:.2}", last_val.unwrap()));
                } else {
                    columns[1].label("N/A");
                    columns[2].label("N/A");
                    columns[3].label("N/A");
                    columns[4].label("N/A");
                }
            });
        }
    }
}
