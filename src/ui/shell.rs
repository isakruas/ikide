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

const FONT_SIZE: f32 = 13.0;

/// Bottom terminal panel: a full interactive shell rendered from the PTY's
/// VT100 screen grid, with keystrokes forwarded to the shell.
pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("shell_panel")
        .resizable(true)
        .min_height(140.0)
        .max_height((ctx.screen_rect().height() * 0.7).clamp(180.0, 600.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Terminal").strong());
                let cwd = app
                    .workspace_dir
                    .as_ref()
                    .map(|w| w.display().to_string())
                    .unwrap_or_else(|| "(no folder open)".to_string());
                ui.label(egui::RichText::new(cwd).weak().small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").on_hover_text("Close").clicked() {
                        app.show_shell = false;
                    }
                    if ui.button("◻ Restart").on_hover_text("Kill and restart the shell").clicked() {
                        app.terminal = None;
                    }
                });
            });
            ui.separator();

            // Spawn the shell session on first open.
            if app.terminal.is_none() {
                let repaint = {
                    let ctx = ctx.clone();
                    move || ctx.request_repaint()
                };
                match crate::core::shell::Terminal::spawn(app.workspace_dir.clone(), repaint) {
                    Ok(t) => app.terminal = Some(t),
                    Err(e) => {
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 120), format!("Cannot start terminal: {}", e));
                        return;
                    }
                }
            }
            let term = app.terminal.as_mut().unwrap();

            let font = egui::FontId::monospace(FONT_SIZE);
            let (cell_w, cell_h) = ui.fonts(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));

            // Fit the grid to the available area and resize the PTY to match.
            let avail = ui.available_size();
            let cols = (avail.x / cell_w).floor().clamp(20.0, 400.0) as u16;
            let rows = (avail.y / cell_h).floor().clamp(4.0, 200.0) as u16;
            term.resize(rows, cols);

            let (resp, painter) = ui.allocate_painter(
                egui::vec2(cols as f32 * cell_w, rows as f32 * cell_h),
                egui::Sense::click(),
            );
            if resp.clicked() {
                resp.request_focus();
            }
            let focused = resp.has_focus();
            if focused {
                // Keep Tab / arrows / Esc inside the terminal instead of moving focus.
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        resp.id,
                        egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                    )
                });
            }

            // Draw the screen grid.
            let origin = resp.rect.min;
            let default_fg = ui.visuals().text_color();
            let default_bg = ui.visuals().extreme_bg_color;
            let parser = term.parser();
            if let Ok(guard) = parser.lock() {
                let screen = guard.screen();
                for row in 0..rows {
                    for col in 0..cols {
                        let Some(cell) = screen.cell(row, col) else { continue };
                        let x = origin.x + col as f32 * cell_w;
                        let y = origin.y + row as f32 * cell_h;
                        let mut fg = conv(cell.fgcolor(), default_fg);
                        let mut bg = conv_opt(cell.bgcolor());
                        if cell.inverse() {
                            // Swap foreground and background (default bg fills in).
                            let new_bg = fg;
                            fg = bg.unwrap_or(default_bg);
                            bg = Some(new_bg);
                        }
                        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
                        if let Some(bg) = bg {
                            painter.rect_filled(rect, 0.0, bg);
                        }
                        let glyph = cell.contents();
                        if !glyph.is_empty() {
                            painter.text(egui::pos2(x, y), egui::Align2::LEFT_TOP, glyph, font.clone(), fg);
                        }
                    }
                }
                if !screen.hide_cursor() {
                    let (cr, cc) = screen.cursor_position();
                    let x = origin.x + cc as f32 * cell_w;
                    let y = origin.y + cr as f32 * cell_h;
                    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
                    let color = if focused {
                        egui::Color32::from_rgba_unmultiplied(180, 180, 180, 130)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(120, 120, 120, 80)
                    };
                    painter.rect_filled(rect, 0.0, color);
                }
            }

            if !focused {
                painter.text(
                    resp.rect.left_bottom() + egui::vec2(4.0, -4.0),
                    egui::Align2::LEFT_BOTTOM,
                    "click to focus",
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(120),
                );
            }

            // Forward keystrokes to the shell.
            if focused {
                let mut bytes: Vec<u8> = Vec::new();
                ui.input(|i| {
                    for ev in &i.events {
                        match ev {
                            egui::Event::Text(t) => bytes.extend_from_slice(t.as_bytes()),
                            egui::Event::Paste(t) => bytes.extend_from_slice(t.as_bytes()),
                            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                                translate_key(*key, *modifiers, &mut bytes)
                            }
                            _ => {}
                        }
                    }
                });
                if !bytes.is_empty() {
                    term.send(&bytes);
                    ctx.request_repaint();
                }
            }
        });
}

/// Translate a non-text key press into the bytes a terminal expects.
fn translate_key(key: egui::Key, m: egui::Modifiers, out: &mut Vec<u8>) {
    // Ctrl+letter -> ASCII control code (Ctrl+C = 0x03, etc.).
    if (m.ctrl || m.command) && !m.alt {
        let name = key.name();
        if name.len() == 1 {
            let c = name.as_bytes()[0];
            if c.is_ascii_alphabetic() {
                out.push(c & 0x1f);
                return;
            }
        }
    }
    match key {
        egui::Key::Enter => out.push(b'\r'),
        egui::Key::Backspace => out.push(0x7f),
        egui::Key::Tab => out.push(b'\t'),
        egui::Key::Escape => out.push(0x1b),
        egui::Key::ArrowUp => out.extend_from_slice(b"\x1b[A"),
        egui::Key::ArrowDown => out.extend_from_slice(b"\x1b[B"),
        egui::Key::ArrowRight => out.extend_from_slice(b"\x1b[C"),
        egui::Key::ArrowLeft => out.extend_from_slice(b"\x1b[D"),
        egui::Key::Home => out.extend_from_slice(b"\x1b[H"),
        egui::Key::End => out.extend_from_slice(b"\x1b[F"),
        egui::Key::Delete => out.extend_from_slice(b"\x1b[3~"),
        egui::Key::PageUp => out.extend_from_slice(b"\x1b[5~"),
        egui::Key::PageDown => out.extend_from_slice(b"\x1b[6~"),
        _ => {}
    }
}

fn conv(c: vt100::Color, default: egui::Color32) -> egui::Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => ansi256(i),
        vt100::Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    }
}

fn conv_opt(c: vt100::Color) -> Option<egui::Color32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(ansi256(i)),
        vt100::Color::Rgb(r, g, b) => Some(egui::Color32::from_rgb(r, g, b)),
    }
}

/// The standard xterm 256-colour palette.
fn ansi256(i: u8) -> egui::Color32 {
    let (r, g, b) = match i {
        0 => (0, 0, 0),
        1 => (205, 0, 0),
        2 => (0, 205, 0),
        3 => (205, 205, 0),
        4 => (0, 0, 238),
        5 => (205, 0, 205),
        6 => (0, 205, 205),
        7 => (229, 229, 229),
        8 => (127, 127, 127),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (92, 92, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let i = i - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let lvl = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            (lvl(r), lvl(g), lvl(b))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    };
    egui::Color32::from_rgb(r, g, b)
}
