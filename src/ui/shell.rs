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
/// Fixed dark backdrop and light default text so the ANSI palette stays legible
/// regardless of the IDE theme.
const TERM_BG: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x1e, 0x1e);
const TERM_FG: egui::Color32 = egui::Color32::from_rgb(0xd6, 0xd6, 0xd6);

/// Bottom terminal panel: tabbed interactive shells rendered from each PTY's
/// VT100 screen grid, with keystrokes forwarded to the active shell.
pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("shell_panel")
        .resizable(true)
        .min_height(140.0)
        .max_height((ctx.screen_rect().height() * 0.7).clamp(180.0, 600.0))
        .show(ctx, |ui| {
            // Open the first session when the panel appears.
            if app.terminals.is_empty() {
                spawn_terminal(app, ctx);
            }
            if app.terminals.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), "Cannot start a terminal.");
                return;
            }
            if app.active_terminal >= app.terminals.len() {
                app.active_terminal = app.terminals.len() - 1;
            }

            let mut to_close: Option<usize> = None;
            let mut add = false;
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖").on_hover_text("Hide panel").clicked() {
                        app.show_shell = false;
                    }
                    if ui.button("+").on_hover_text("New terminal").clicked() {
                        add = true;
                    }
                    // Remaining width, left to right: title + scrollable tab strip.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("Terminal").strong());
                        ui.separator();
                        egui::ScrollArea::horizontal()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for i in 0..app.terminals.len() {
                                        if i > 0 {
                                            ui.separator();
                                        }
                                        if app.renaming_terminal == Some(i) {
                                            // Inline title editor (double-click or Rename).
                                            let resp = ui.add(
                                                egui::TextEdit::singleline(&mut app.terminal_rename_buf)
                                                    .desired_width(110.0),
                                            );
                                            resp.request_focus();
                                            let commit = resp.lost_focus()
                                                || ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                                            if commit {
                                                let name = app.terminal_rename_buf.trim();
                                                if !name.is_empty() {
                                                    app.terminals[i].title = name.to_string();
                                                }
                                                app.renaming_terminal = None;
                                            }
                                        } else {
                                            let selected = i == app.active_terminal;
                                            let resp = ui.selectable_label(selected, &app.terminals[i].title);
                                            if resp.clicked() {
                                                app.active_terminal = i;
                                            }
                                            if resp.double_clicked() {
                                                app.renaming_terminal = Some(i);
                                                app.terminal_rename_buf = app.terminals[i].title.clone();
                                            }
                                            resp.context_menu(|ui| {
                                                if ui.button("Rename").clicked() {
                                                    app.renaming_terminal = Some(i);
                                                    app.terminal_rename_buf = app.terminals[i].title.clone();
                                                    ui.close_menu();
                                                }
                                                if ui.button("Close").clicked() {
                                                    app.terminal_close_confirm = Some(i);
                                                    ui.close_menu();
                                                }
                                            });
                                            if ui.small_button("×").on_hover_text("Close terminal").clicked() {
                                                app.terminal_close_confirm = Some(i);
                                            }
                                        }
                                    }
                                });
                            });
                    });
                });
            });
            ui.separator();

            // Confirm before killing a shell, to guard against accidental clicks.
            if let Some(i) = app.terminal_close_confirm {
                if i >= app.terminals.len() {
                    app.terminal_close_confirm = None;
                } else {
                    let title = app.terminals[i].title.clone();
                    egui::Window::new("close_terminal_confirm")
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(egui::RichText::new("Close terminal").strong());
                            ui.label(format!("Close \"{}\"? Its running shell will be terminated.", title));
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui.button("Close").clicked() {
                                    to_close = Some(i);
                                    app.terminal_close_confirm = None;
                                }
                                if ui.button("Cancel").clicked() {
                                    app.terminal_close_confirm = None;
                                }
                            });
                        });
                }
            }

            if add {
                spawn_terminal(app, ctx);
            }
            if let Some(i) = to_close {
                app.renaming_terminal = None;
                app.terminals.remove(i); // Drop kills the child process.
                if app.terminals.is_empty() {
                    app.show_shell = false;
                    return;
                }
                if app.active_terminal >= app.terminals.len() {
                    app.active_terminal = app.terminals.len() - 1;
                }
            }

            render_active(app, ctx, ui);
        });
}

/// Create a new terminal tab and make it active.
fn spawn_terminal(app: &mut IkIdeApp, ctx: &egui::Context) {
    app.next_terminal_id += 1;
    let title = format!("Terminal {}", app.next_terminal_id);
    let repaint = {
        let ctx = ctx.clone();
        move || ctx.request_repaint()
    };
    if let Ok(t) = crate::core::shell::Terminal::spawn(app.workspace_dir.clone(), title, repaint) {
        app.terminals.push(t);
        app.active_terminal = app.terminals.len() - 1;
    }
}

/// Draw the active terminal's grid and forward input to it.
fn render_active(app: &mut IkIdeApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    let term = &mut app.terminals[app.active_terminal];

    let font = egui::FontId::monospace(FONT_SIZE);
    let (cell_w, cell_h) = ui.fonts(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));

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

    // Dark backdrop, then the cells over it.
    painter.rect_filled(resp.rect, 0.0, TERM_BG);
    let origin = resp.rect.min;
    let parser = term.parser();
    if let Ok(guard) = parser.lock() {
        let screen = guard.screen();
        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else { continue };
                let x = origin.x + col as f32 * cell_w;
                let y = origin.y + row as f32 * cell_h;
                let mut fg = conv(cell.fgcolor(), TERM_FG);
                let mut bg = conv_opt(cell.bgcolor());
                if cell.inverse() {
                    let new_bg = fg;
                    fg = bg.unwrap_or(TERM_BG);
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
                egui::Color32::from_rgba_unmultiplied(210, 210, 210, 140)
            } else {
                egui::Color32::from_rgba_unmultiplied(150, 150, 150, 80)
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
            egui::Color32::from_gray(140),
        );
    }

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

/// The xterm 256-colour palette, with the dim blue lightened so it reads on the
/// dark terminal background.
fn ansi256(i: u8) -> egui::Color32 {
    let (r, g, b) = match i {
        0 => (30, 30, 30),
        1 => (224, 80, 80),
        2 => (120, 200, 120),
        3 => (210, 195, 110),
        4 => (90, 140, 240),
        5 => (210, 120, 210),
        6 => (90, 200, 205),
        7 => (210, 210, 210),
        8 => (127, 127, 127),
        9 => (255, 110, 110),
        10 => (120, 230, 120),
        11 => (240, 230, 120),
        12 => (120, 165, 255),
        13 => (240, 130, 240),
        14 => (110, 230, 235),
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
