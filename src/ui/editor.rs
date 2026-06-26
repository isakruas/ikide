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

use std::collections::{HashMap, HashSet};
use std::time::Instant;
use eframe::egui;
use crate::app::IkIdeApp;
use crate::core::analysis;
use crate::syntax;

pub fn render(app: &mut IkIdeApp, ctx: &egui::Context) {
    // Build per-line lookups from the live diagnostics (compiler + lints). Only
    // diagnostics for the active file drive the gutter/squiggles — a diagnostic
    // resolved into an imported file carries its own `file` and would otherwise
    // mark the wrong line here.
    let active_path = app.active_tab.and_then(|i| app.open_tabs.get(i)).map(|t| t.path.clone());
    let all = app.all_diagnostics();
    let diags: Vec<analysis::Diagnostic> = all
        .into_iter()
        .filter(|d| d.file.is_none() || d.file == active_path)
        .collect();
    let error_terms: HashMap<usize, (String, analysis::Severity)> = analysis::highlight_terms(&diags);
    let error_lines: HashSet<usize> = diags
        .iter()
        .filter(|d| d.line > 0 && d.severity == analysis::Severity::Error)
        .map(|d| d.line)
        .collect();
    let warn_lines: HashSet<usize> = diags
        .iter()
        .filter(|d| d.line > 0 && d.severity == analysis::Severity::Warning)
        .map(|d| d.line)
        .filter(|l| !error_lines.contains(l))
        .collect();

    egui::CentralPanel::default().show(ctx, |ui| {
        if app.open_tabs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No file open");
            });
            return;
        }

        // Tab bar
        egui::ScrollArea::horizontal().id_salt("tabs_scroll").show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut close_tab = None;
                for (idx, tab) in app.open_tabs.iter().enumerate() {
                    let is_active = app.active_tab == Some(idx);
                    let title = tab.path.file_name().unwrap_or_default().to_string_lossy();
                    let display_title = if tab.is_modified {
                        if tab.is_disk_different {
                            format!("{} * ⚠", title)
                        } else {
                            format!("{} *", title)
                        }
                    } else if tab.is_disk_different {
                        format!("{} ⚠", title)
                    } else {
                        title.into_owned()
                    };
                    
                    let response = ui.selectable_label(is_active, display_title);
                    if response.clicked() {
                        app.active_tab = Some(idx);
                    }
                    // Middle click to close tab
                    if response.middle_clicked() {
                        close_tab = Some(idx);
                    }
                    
                    // Add a small X button for closing
                    if ui.button("✖").clicked() {
                        close_tab = Some(idx);
                    }
                    ui.separator();
                }
                
                if let Some(idx) = close_tab {
                    app.open_tabs.remove(idx);
                    if let Some(active) = app.active_tab {
                        if active == idx {
                            app.active_tab = if app.open_tabs.is_empty() { None } else { Some(app.open_tabs.len().saturating_sub(1)) };
                        } else if active > idx {
                            app.active_tab = Some(active - 1);
                        }
                    }
                }
            });
        });
        
        ui.separator();

        if let Some(idx) = app.active_tab {
            if idx < app.open_tabs.len() {
                let tab = &mut app.open_tabs[idx];
                if tab.is_disk_different {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 170, 0), "⚠ File modified externally.");
                        if ui.button("Reload from disk").clicked() {
                            if let Ok(new_content) = std::fs::read_to_string(&tab.path) {
                                tab.content = new_content;
                                tab.is_modified = false;
                                tab.is_disk_different = false;
                                tab.last_mtime = std::fs::metadata(&tab.path).ok().and_then(|m| m.modified().ok());
                            }
                        }
                    });
                    ui.separator();
                }
                // Only ik8b sources get syntax styling and language features;
                // any other file is shown as plain text.
                let is_ik = crate::app::is_ik_file(&tab.path);
                let is_rhai = crate::app::is_rhai_file(&tab.path);

                let theme = syntax::CodeTheme::default();


                let offset_y: f32 = ui.data(|d| d.get_temp(egui::Id::new("editor_y_offset")).unwrap_or(0.0));
                let content_h: f32 = ui.data(|d| d.get_temp(egui::Id::new("editor_content_h")).unwrap_or(1.0));
                let view_h: f32 = ui.data(|d| d.get_temp(egui::Id::new("editor_view_h")).unwrap_or(1.0));

                if app.show_minimap {
                    egui::SidePanel::right("minimap_panel")
                        .resizable(true)
                        .default_width(120.0)
                        .min_width(60.0)
                        .max_width(260.0)
                        .show_inside(ui, |ui| {
                            egui::ScrollArea::both()
                                .id_salt("minimap_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let layout_job = if is_ik {
                                        syntax::highlight(&tab.content, &theme, 3.5, &error_terms, None)
                                    } else if is_rhai {
                                        syntax::highlight_rhai(&tab.content, 3.5)
                                    } else {
                                        syntax::plain_job(&tab.content, 3.5)
                                    };
                                    let label_resp = ui.add(
                                        egui::Label::new(layout_job)
                                            .selectable(false)
                                            .sense(egui::Sense::click_and_drag())
                                    );
                                    
                                    if content_h > 0.0 {
                                        let ratio_y = offset_y / content_h;
                                        let ratio_h = view_h / content_h;
                                        
                                        let rect_y = label_resp.rect.top() + label_resp.rect.height() * ratio_y;
                                        let rect_h = label_resp.rect.height() * ratio_h;
                                        
                                        let highlight_rect = egui::Rect::from_min_size(
                                            egui::pos2(label_resp.rect.left(), rect_y),
                                            egui::vec2(label_resp.rect.width(), rect_h)
                                        );
                                        
                                        ui.painter().rect_filled(highlight_rect, 0.0, egui::Color32::from_white_alpha(30));
                                        
                                        if label_resp.dragged() || label_resp.clicked() {
                                            if let Some(pos) = label_resp.interact_pointer_pos() {
                                                let local_y = pos.y - label_resp.rect.top();
                                                let minimap_h = label_resp.rect.height();
                                                let target_ratio = local_y / minimap_h;
                                                
                                                let target_offset = (target_ratio * content_h) - (view_h / 2.0);
                                                let target_offset = target_offset.clamp(0.0, (content_h - view_h).max(0.0));
                                                
                                                ui.data_mut(|d| d.insert_temp(egui::Id::new("editor_force_scroll"), target_offset));
                                            }
                                        }
                                    }
                                });
                        });
                }

                // A pending "go to line" (e.g. from clicking a problem) wins over
                // the minimap's pixel offset. Convert the 1-based line to a pixel
                // offset via the monospace row height so it works the same frame a
                // different file was opened (content height may still be stale).
                let force_scroll: Option<f32> = if let Some(line) = app.pending_scroll_line.take() {
                    let row_h = ui.fonts(|f| f.row_height(&egui::FontId::monospace(14.0)));
                    let target = (line.saturating_sub(1) as f32) * row_h - view_h / 2.0;
                    Some(target.max(0.0))
                } else {
                    ui.data_mut(|d| {
                        let val = d.get_temp(egui::Id::new("editor_force_scroll"));
                        if val.is_some() {
                            d.remove_temp::<f32>(egui::Id::new("editor_force_scroll"));
                        }
                        val
                    })
                };

                let mut editor_scroll = egui::ScrollArea::both().id_salt("editor_scroll");
                if let Some(target) = force_scroll {
                    editor_scroll = editor_scroll.vertical_scroll_offset(target);
                }

                let output = editor_scroll.show(ui, |ui| {
                    // Top-align the row so the line-number column and the code
                    // share the same first-row baseline (default horizontal()
                    // centers them, which misaligns when their heights differ).
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        // Reserve the line-number gutter; the numbers are painted
                        // after layout, aligned to the galley's visual rows. Each
                        // logical line is numbered at its first visual row, leaving
                        // wrapped continuation rows blank.
                        let logical_total = tab.content.lines().count().max(1)
                            + if tab.content.ends_with('\n') { 1 } else { 0 };
                        let gutter_digits = logical_total.to_string().len().max(3);
                        let gutter_font = egui::FontId::monospace(14.0);
                        let gutter_char_w = ui.fonts(|f| f.glyph_width(&gutter_font, '0'));
                        let gutter_w = gutter_char_w * gutter_digits as f32 + 6.0;
                        let gutter_x = ui.cursor().min.x;
                        ui.add_space(gutter_w + 8.0); // column + gap for the divider

                        // Key the editor (and thus its undo/redo and cursor state)
                        // by file path, so each tab has its own independent history.
                        let text_edit_id = ui.id().with("editor_multiline").with(&tab.path);
                        let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                            let mut selection = None;
                            if let Some(state) = egui::TextEdit::load_state(ui.ctx(), text_edit_id) {
                                if let Some(range) = state.cursor.char_range() {
                                    let min_idx = range.primary.index.min(range.secondary.index);
                                    let max_idx = range.primary.index.max(range.secondary.index);
                                    if min_idx != max_idx {
                                        selection = Some((min_idx, max_idx));
                                    }
                                }
                            }
                            let mut layout_job = if is_ik {
                                syntax::highlight(string, &theme, 14.0, &error_terms, selection)
                            } else if is_rhai {
                                syntax::highlight_rhai(string, 14.0)
                            } else {
                                syntax::plain_job(string, 14.0)
                            };
                            layout_job.wrap.max_width = wrap_width;
                            ui.fonts(|f| f.layout_job(layout_job))
                        };

                        // Indentation handling: Shift+Tab dedents the touched lines;
                        // Tab indents them when a multi-line block is selected. A
                        // single-caret Tab is left to the editor, which inserts 4
                        // spaces.
                        if ui.memory(|m| m.has_focus(text_edit_id)) {
                            let sel = egui::TextEdit::load_state(ui.ctx(), text_edit_id)
                                .and_then(|s| s.cursor.char_range())
                                .map(|r| {
                                    (
                                        r.primary.index.min(r.secondary.index),
                                        r.primary.index.max(r.secondary.index),
                                    )
                                });
                            if let Some((smin, smax)) = sel {
                                let multiline =
                                    tab.content.chars().skip(smin).take(smax - smin).any(|c| c == '\n');
                                let dedent = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
                                let indent = !dedent
                                    && multiline
                                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                                if dedent || indent {
                                    let (new_text, nmin, nmax) = reindent(&tab.content, smin, smax, dedent);
                                    if new_text != tab.content {
                                        tab.content = new_text;
                                        tab.is_modified = true;
                                        app.last_edit = Some(Instant::now());
                                    }
                                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), text_edit_id) {
                                        let primary = egui::text::CCursor::new(nmax);
                                        let secondary = egui::text::CCursor::new(nmin);
                                        state.cursor.set_char_range(Some(egui::text::CCursorRange { primary, secondary }));
                                        egui::TextEdit::store_state(ui.ctx(), text_edit_id, state);
                                    }
                                }
                            }
                        }

                        let mut text = tab.content.clone();
                        let text_edit_output = egui::TextEdit::multiline(&mut text)
                            .id(text_edit_id)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .frame(false) // removes the dark background block
                            .desired_width(f32::INFINITY)
                            .margin(egui::vec2(0.0, 0.0)) // Disable all margin for perfect alignment
                            .layouter(&mut layouter)
                            .show(ui);

                        // Paint the gutter from the laid-out galley: number each
                        // logical line at its first visual row, leaving wrapped
                        // continuation rows blank, with a divider on the right edge.
                        {
                            let galley = &text_edit_output.galley;
                            let gpos = text_edit_output.galley_pos;
                            let painter = ui.painter();
                            let div_x = gutter_x + gutter_w + 4.0;
                            painter.line_segment(
                                [egui::pos2(div_x, gpos.y), egui::pos2(div_x, gpos.y + galley.rect.height())],
                                ui.visuals().widgets.noninteractive.bg_stroke,
                            );
                            let mut logical = 0usize;
                            for (r, row) in galley.rows.iter().enumerate() {
                                let is_line_start = r == 0 || galley.rows[r - 1].ends_with_newline;
                                if !is_line_start {
                                    continue;
                                }
                                logical += 1;
                                let y = gpos.y + row.rect.min.y;
                                let (color, bg) = if error_lines.contains(&logical) {
                                    (egui::Color32::RED, Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 50)))
                                } else if warn_lines.contains(&logical) {
                                    (egui::Color32::from_rgb(220, 150, 0), Some(egui::Color32::from_rgba_unmultiplied(220, 150, 0, 50)))
                                } else {
                                    (egui::Color32::from_gray(100), None)
                                };
                                if let Some(bg) = bg {
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(egui::pos2(gutter_x, y), egui::vec2(gutter_w, row.rect.height())),
                                        0.0,
                                        bg,
                                    );
                                }
                                painter.text(
                                    egui::pos2(gutter_x + gutter_w, y),
                                    egui::Align2::RIGHT_TOP,
                                    logical.to_string(),
                                    gutter_font.clone(),
                                    color,
                                );
                            }
                        }

                        let response = text_edit_output.response;

                        if response.changed() {
                            if text.contains('\t') {
                                // Tab inserts 4 spaces. A plain global replace would
                                // leave the cursor 3 chars behind each converted tab,
                                // so move it to *after* the inserted spaces.
                                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                                    let cursor = state.cursor.char_range()
                                        .map(|r| r.primary.index)
                                        .unwrap_or_else(|| text.chars().count());
                                    let tabs_before = text.chars().take(cursor).filter(|&c| c == '\t').count();
                                    text = text.replace('\t', "    ");
                                    let c = egui::text::CCursor::new(cursor + tabs_before * 3);
                                    state.cursor.set_char_range(Some(egui::text::CCursorRange { primary: c, secondary: c }));
                                    egui::TextEdit::store_state(ui.ctx(), response.id, state);
                                } else {
                                    text = text.replace('\t', "    ");
                                }
                            }
                            tab.content = text.clone();
                            tab.is_modified = true;
                            // Restart the live type-check debounce timer.
                            app.last_edit = Some(Instant::now());
                        }

                        if let Some(state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                            if let Some(range) = state.cursor.char_range() {
                                let cursor_idx = range.primary.index;
                                let text_up_to_cursor: String = text.chars().take(cursor_idx).collect();
                                let force_autocomplete = is_ik && ui.input(|i| app.keymap.autocomplete.pressed(i));
                                let key = egui::Id::new("autocomplete_sugs");
                                let is_sug_open = ui.data_mut(|d| d.get_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(key)).is_some();

                                if !is_ik {
                                    // Plain-text file: never offer code completion.
                                    ui.data_mut(|d| d.remove_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(key));
                                } else if force_autocomplete || is_sug_open {
                                    // Split the prefix into tokens to find the word under the
                                    // cursor and the token before it (for context completions).
                                    let parts: Vec<&str> = text_up_to_cursor
                                        .split(|c: char| c.is_whitespace() || "{}[]()".contains(c))
                                        .collect();
                                    let word = parts.last().copied().unwrap_or("");
                                    let nonempty: Vec<&str> = parts.iter().copied().filter(|s| !s.is_empty()).collect();
                                    let prev_word = if word.is_empty() {
                                        nonempty.last().copied().unwrap_or("")
                                    } else {
                                        nonempty.get(nonempty.len().saturating_sub(2)).copied().unwrap_or("")
                                    };

                                    let index = app.std_index.with_buffer(&text);
                                    let items = analysis::completions(&index, word, prev_word, &app.devices, force_autocomplete || is_sug_open);
                                    if items.is_empty() {
                                        ui.data_mut(|d| d.remove_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(key));
                                    } else {
                                        ui.data_mut(|d| d.insert_temp(key, (word.chars().count(), cursor_idx, items)));
                                    }
                                }
                            }
                        }

                        // Close popup if Escape is pressed
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            ui.data_mut(|d| d.remove_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(egui::Id::new("autocomplete_sugs")));
                        }

                        // Detect a selected single word so a keyword reference card
                        // can be shown for it (e.g. select `flash` or `switch`).
                        let mut selected: Option<(usize, usize, String)> = None;
                        if let Some(state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                            if let Some(range) = state.cursor.char_range() {
                                let a = range.primary.index.min(range.secondary.index);
                                let b = range.primary.index.max(range.secondary.index);
                                if is_ik && b > a && b - a <= 12 {
                                    let sel: String = text.chars().skip(a).take(b - a).collect();
                                    selected = Some((a, b, sel.trim().to_string()));
                                }
                            }
                        }
                        ui.data_mut(|d| match selected {
                            Some(t) => d.insert_temp(egui::Id::new("kw_help_word"), t),
                            None => {
                                d.remove_temp::<(usize, usize, String)>(egui::Id::new("kw_help_word"));
                            }
                        });

                        // A broader (multi-char) selection feeds the code-assistant
                        // action bar. Unlike the keyword card it isn't length-capped.
                        let mut snippet: Option<(usize, usize, String)> = None;
                        if let Some(state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                            if let Some(range) = state.cursor.char_range() {
                                let a = range.primary.index.min(range.secondary.index);
                                let b = range.primary.index.max(range.secondary.index);
                                if is_ik && b > a {
                                    let sel: String = text.chars().skip(a).take(b - a).collect();
                                    if !sel.trim().is_empty() {
                                        snippet = Some((a, b, sel));
                                    }
                                }
                            }
                        }
                        ui.data_mut(|d| match snippet {
                            Some(t) => d.insert_temp(egui::Id::new("ai_sel_snippet"), t),
                            None => {
                                d.remove_temp::<(usize, usize, String)>(egui::Id::new("ai_sel_snippet"));
                            }
                        });

                        if let Some(cursor_range) = text_edit_output.cursor_range {
                            let rect = text_edit_output.galley.pos_from_cursor(&cursor_range.primary);
                            let screen_pos = text_edit_output.galley_pos + rect.min.to_vec2();
                            ui.data_mut(|d| {
                                d.insert_temp(egui::Id::new("cursor_screen_pos"), screen_pos);
                                d.insert_temp(egui::Id::new("cursor_idx"), cursor_range.primary.ccursor.index);
                            });
                        }
                    });
                });
                
                let key = egui::Id::new("autocomplete_sugs");
                let sugs: Option<(usize, usize, Vec<analysis::CompletionItem>)> = ui.data_mut(|d| d.get_temp(key));
                if let Some((word_len, cursor_idx, items)) = sugs {
                    let mut pos = ui.data_mut(|d| d.get_temp::<egui::Pos2>(egui::Id::new("cursor_screen_pos"))).unwrap_or(ctx.pointer_latest_pos().unwrap_or(egui::pos2(100.0, 100.0)));
                    pos.y += 20.0;
                    egui::Window::new("Assistant")
                        .title_bar(false)
                        .resizable(false)
                        .collapsible(false)
                        .fixed_pos(pos)
                        .show(ctx, |ui| {
                            ui.label(egui::RichText::new("💡 Suggestions").strong().color(egui::Color32::YELLOW));
                            ui.separator();
                            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                                for item in &items {
                                    let clicked = ui.horizontal(|ui| {
                                        let mut btn = ui.button(
                                            egui::RichText::new(&item.label).monospace().color(egui::Color32::from_rgb(100, 200, 255))
                                        );
                                        if !item.doc.is_empty() {
                                            btn = btn.on_hover_text(&item.doc);
                                        }
                                        if !item.detail.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&item.detail).monospace().color(egui::Color32::from_gray(150))
                                            );
                                        }
                                        btn.clicked()
                                    }).inner;

                                    if clicked {
                                        let start_idx = cursor_idx.saturating_sub(word_len);
                                        let prefix: String = tab.content.chars().take(start_idx).collect();
                                        let suffix: String = tab.content.chars().skip(cursor_idx).collect();
                                        tab.content = format!("{}{}{}", prefix, item.insert, suffix);
                                        tab.is_modified = true;
                                        app.last_edit = Some(Instant::now());
                                        ui.data_mut(|d| d.remove_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(key));
                                    }
                                }
                            });
                        });
                }

                // Keyword reference card: shown when a single keyword/type is
                // selected and no completion popup is open.
                let autocomplete_open = ui
                    .data_mut(|d| d.get_temp::<(usize, usize, Vec<analysis::CompletionItem>)>(egui::Id::new("autocomplete_sugs")))
                    .is_some();
                // Precedence near the cursor so popups never overlap:
                // autocomplete > keyword/snippet card > AI action bar. The card is
                // shown when a recognised keyword is selected; the AI bar yields to it.
                let kw_help_active = !autocomplete_open
                    && ui
                        .data_mut(|d| d.get_temp::<(usize, usize, String)>(egui::Id::new("kw_help_word")))
                        .map_or(false, |(_, _, w)| analysis::keyword_help(&w).is_some());
                if !autocomplete_open {
                    if let Some((sel_start, sel_end, word)) = ui.data_mut(|d| d.get_temp::<(usize, usize, String)>(egui::Id::new("kw_help_word"))) {
                        if let Some(help) = analysis::keyword_help(&word) {
                            let mut pos = ui
                                .data_mut(|d| d.get_temp::<egui::Pos2>(egui::Id::new("cursor_screen_pos")))
                                .unwrap_or(egui::pos2(120.0, 120.0));
                            pos.y += 20.0;
                            egui::Window::new("KeywordHelp")
                                .title_bar(false)
                                .resizable(false)
                                .collapsible(false)
                                .fixed_pos(pos)
                                .show(ctx, |ui| {
                                    ui.set_max_width(380.0);
                                    ui.label(egui::RichText::new(&help.title).strong().color(egui::Color32::from_rgb(120, 200, 255)));
                                    ui.separator();
                                    ui.label(egui::RichText::new(&help.doc).small());
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new("Example").small().weak());
                                    let theme = syntax::CodeTheme::default();
                                    let empty: HashMap<usize, (String, analysis::Severity)> = HashMap::new();
                                    let job = syntax::highlight(&help.snippet, &theme, 13.0, &empty, None);
                                    egui::Frame::none()
                                        .fill(egui::Color32::from_gray(28))
                                        .inner_margin(egui::Margin::same(6.0))
                                        .show(ui, |ui| {
                                            ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                                        });
                                    ui.add_space(6.0);
                                    // Insert the example, replacing the selected keyword.
                                    if ui.button(egui::RichText::new("📋 Insert example").color(egui::Color32::from_rgb(120, 220, 140))).clicked() {
                                        let prefix: String = tab.content.chars().take(sel_start).collect();
                                        let suffix: String = tab.content.chars().skip(sel_end).collect();
                                        tab.content = format!("{}{}{}", prefix, help.snippet, suffix);
                                        tab.is_modified = true;
                                        app.last_edit = Some(Instant::now());
                                        ui.data_mut(|d| d.remove_temp::<(usize, usize, String)>(egui::Id::new("kw_help_word")));
                                    }
                                });
                        }
                    }
                }

                // Code-assistant action bar: a snippet selected in an .ik file (with
                // the agent enabled as code assistant) floats a toolbar near the
                // selection to edit or explain it. Hidden while autocomplete or the
                // keyword card occupies the cursor position.
                if app.agent_as_code_assistant && !autocomplete_open && !kw_help_active {
                    let snippet: Option<String> = ui
                        .data_mut(|d| d.get_temp::<(usize, usize, String)>(egui::Id::new("ai_sel_snippet")))
                        .map(|(_, _, s)| s)
                        .filter(|s| !s.trim().is_empty());
                    if let Some(snippet) = snippet {
                        let mut pos = ui
                            .data_mut(|d| d.get_temp::<egui::Pos2>(egui::Id::new("cursor_screen_pos")))
                            .unwrap_or(egui::pos2(120.0, 120.0));
                        pos.y += 20.0;
                        egui::Window::new("AiSelectionActions")
                            .title_bar(false)
                            .resizable(false)
                            .collapsible(false)
                            .fixed_pos(pos)
                            .show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    let busy = app.ai_agent_running;
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("AI Edit"))
                                        .on_hover_text("Ask the agent to rewrite the selected code")
                                        .clicked()
                                    {
                                        app.ai_edit_selection = Some(snippet.clone());
                                        app.ai_edit_open = true;
                                    }
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("Explain"))
                                        .on_hover_text("Ask the agent to explain it in the AI chat")
                                        .clicked()
                                    {
                                        let prompt = format!(
                                            "Explain this ik code:\n\n```\n{}\n```",
                                            snippet
                                        );
                                        app.start_agent_turn(prompt, ctx);
                                    }
                                });
                            });
                    }
                }

                ui.data_mut(|d| {
                    d.insert_temp(egui::Id::new("editor_y_offset"), output.state.offset.y);
                    d.insert_temp(egui::Id::new("editor_content_h"), output.content_size.y);
                    d.insert_temp(egui::Id::new("editor_view_h"), output.inner_rect.height());
                });
            }
        }
    });
}

/// Indent or dedent every line touched by the selection `[sel_min, sel_max)` by
/// one 4-space step. Returns the new text and the adjusted (min, max) selection,
/// both as char indices. Dedent removes up to 4 leading spaces (or one tab) per
/// line; indent prepends 4 spaces to each non-empty line.
fn reindent(content: &str, sel_min: usize, sel_max: usize, dedent: bool) -> (String, usize, usize) {
    const STEP: usize = 4;
    let chars: Vec<char> = content.chars().collect();
    let n = chars.len();
    let smin = sel_min.min(n);
    let smax = sel_max.min(n);

    // Start of the first line the selection touches.
    let mut first = smin;
    while first > 0 && chars[first - 1] != '\n' {
        first -= 1;
    }
    // A selection ending exactly at a line start doesn't include that line.
    let end = if smax > smin && smax > 0 && chars[smax - 1] == '\n' {
        smax - 1
    } else {
        smax
    };

    let mut out: Vec<char> = chars[..first].to_vec();
    let mut i = first;
    let mut dmin: isize = 0;
    let mut dmax: isize = 0;
    loop {
        if dedent {
            let mut removed = 0usize;
            let mut j = i;
            if j < n && chars[j] == '\t' {
                j += 1;
                removed = 1;
            } else {
                while removed < STEP && j < n && chars[j] == ' ' {
                    j += 1;
                    removed += 1;
                }
            }
            if i < smin {
                dmin -= removed.min(smin - i) as isize;
            }
            if i <= smax {
                dmax -= removed.min(smax.saturating_sub(i)) as isize;
            }
            let mut k = j;
            while k < n && chars[k] != '\n' {
                k += 1;
            }
            if k < n {
                k += 1;
            }
            out.extend_from_slice(&chars[j..k]);
            i = k;
        } else {
            // Indent: blank lines are skipped to avoid trailing whitespace.
            if i < n && chars[i] != '\n' {
                for _ in 0..STEP {
                    out.push(' ');
                }
                if i < smin {
                    dmin += STEP as isize;
                }
                if i <= smax {
                    dmax += STEP as isize;
                }
            }
            let mut k = i;
            while k < n && chars[k] != '\n' {
                k += 1;
            }
            if k < n {
                k += 1;
            }
            out.extend_from_slice(&chars[i..k]);
            i = k;
        }
        if i > end || i >= n {
            break;
        }
    }
    out.extend_from_slice(&chars[i.min(n)..]);

    let new_min = (smin as isize + dmin).max(0) as usize;
    let new_max = (smax as isize + dmax).max(0) as usize;
    (out.into_iter().collect(), new_min, new_max)
}

#[cfg(test)]
mod reindent_tests {
    use super::reindent;

    #[test]
    fn dedent_single_line_removes_four_spaces() {
        let (t, _, _) = reindent("    foo", 4, 4, true);
        assert_eq!(t, "foo");
    }

    #[test]
    fn dedent_tab_removes_one_tab() {
        let (t, _, _) = reindent("\tfoo", 1, 1, true);
        assert_eq!(t, "foo");
    }

    #[test]
    fn indent_multiline_selection_prefixes_each_line() {
        // select across both lines
        let src = "a\nb\n";
        let (t, _, _) = reindent(src, 0, 4, false);
        assert_eq!(t, "    a\n    b\n");
    }

    #[test]
    fn dedent_multiline_only_strips_present_indent() {
        let src = "    a\n  b\n";
        let (t, _, _) = reindent(src, 0, src.chars().count(), true);
        assert_eq!(t, "a\nb\n");
    }

    #[test]
    fn indent_skips_blank_lines() {
        let src = "a\n\nb";
        let (t, _, _) = reindent(src, 0, src.chars().count(), false);
        assert_eq!(t, "    a\n\n    b");
    }
}
