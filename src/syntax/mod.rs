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
use std::collections::HashMap;

#[derive(Default)]
pub struct CodeTheme {
    // Extensible for future custom colors
}

/// A plain, unstyled monospace layout — used for non-`.ik` files, which the
/// IDE opens as plain text without any ik8b syntax styling.
pub fn plain_job(code: &str, font_size: f32) -> egui::text::LayoutJob {
    egui::text::LayoutJob::simple(
        code.to_owned(),
        egui::FontId::monospace(font_size),
        egui::Color32::from_gray(220),
        f32::INFINITY,
    )
}

/// Highlight `code`. `error_terms` maps a 1-based line number to the offending
/// identifier reported by the compiler on that line; matching tokens are drawn
/// with an error background so every diagnostic is visible at once.
pub fn highlight(code: &str, _theme: &CodeTheme, font_size: f32, error_terms: &HashMap<usize, String>, selection: Option<(usize, usize)>) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut current_line = 1;
    let mut char_idx = 0;
    
    let keywords = ["target", "import", "const", "mut", "imut", "ram", "eeprom", "flash", "return", "loop", "switch", "ptr", "str", "fn", "isr", "boot"];
    let types = ["u8", "u16", "i8", "i16", "bool", "char", "r8", "r16", "void"];
    let booleans = ["true", "false"];

    let font_id = egui::FontId::monospace(font_size);
    
    let mut rest = code;
    while !rest.is_empty() {
        if rest.starts_with('#') {
            // Comment
            let end = rest.find('\n').unwrap_or(rest.len());
            let (comment, new_rest) = rest.split_at(end);
            current_line += comment.chars().filter(|&c| c == '\n').count();
            char_idx += comment.chars().count();
            job.append(comment, 0.0, egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(100, 150, 100)));
            rest = new_rest;
        } else if rest.starts_with('"') || rest.starts_with('\'') {
            // String literal
            let quote = rest.chars().next().unwrap();
            let mut end = 1;
            let mut escape = false;
            for (i, c) in rest[1..].char_indices() {
                end = i + 1;
                if escape {
                    escape = false;
                } else if c == '\\' {
                    escape = true;
                } else if c == quote {
                    end = i + 2;
                    break;
                }
            }
            if end == rest.len() - 1 && !rest.ends_with(quote) { end = rest.len(); }
            
            let (str_lit, new_rest) = rest.split_at(end);
            current_line += str_lit.chars().filter(|&c| c == '\n').count();
            char_idx += str_lit.chars().count();
            job.append(str_lit, 0.0, egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(200, 150, 100)));
            rest = new_rest;
        } else if let Some(first_char) = rest.chars().next() {
            if first_char.is_alphabetic() || first_char == '_' {
                // Identifier or keyword
                let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
                let (ident, new_rest) = rest.split_at(end);
                let mut format = if keywords.contains(&ident) {
                    egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(200, 100, 200))
                } else if types.contains(&ident) {
                    egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(100, 200, 200))
                } else if booleans.contains(&ident) {
                    egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(200, 150, 100))
                } else {
                    egui::TextFormat::simple(font_id.clone(), egui::Color32::LIGHT_GRAY)
                };
                
                if let Some(term) = error_terms.get(&current_line) {
                    if !term.is_empty() && ident == term {
                        format.color = egui::Color32::WHITE;
                        format.background = egui::Color32::RED;
                        format.underline = egui::Stroke::new(2.0, egui::Color32::RED);
                    }
                }
                
                job.append(ident, 0.0, format);
                char_idx += ident.chars().count();
                rest = new_rest;
            } else if first_char.is_numeric() {
                // Number
                let end = if rest.starts_with("0x") {
                    rest[2..].find(|c: char| !c.is_ascii_hexdigit()).map(|i| i + 2).unwrap_or(rest.len())
                } else {
                    rest.find(|c: char| !c.is_numeric() && c != '.').unwrap_or(rest.len())
                };
                let (num, new_rest) = rest.split_at(end);
                current_line += num.chars().filter(|&c| c == '\n').count();
                char_idx += num.chars().count();
                job.append(num, 0.0, egui::TextFormat::simple(font_id.clone(), egui::Color32::from_rgb(150, 200, 150)));
                rest = new_rest;
            } else if first_char == '@' || first_char == '$' || first_char == '%' {
                // Variable, function or register
                let end = rest[1..].find(|c: char| !c.is_alphanumeric() && c != '_').map(|i| i + 1).unwrap_or(rest.len());
                let (var, new_rest) = rest.split_at(end);
                let base = if first_char == '@' {
                    egui::Color32::from_rgb(100, 200, 255)
                } else if first_char == '%' {
                    egui::Color32::from_rgb(255, 150, 100)
                } else {
                    egui::Color32::from_rgb(255, 200, 100)
                };
                let mut format = egui::TextFormat::simple(font_id.clone(), base);
                // Sigiled tokens (@fn, $var, %reg) can be the offending term too.
                if let Some(term) = error_terms.get(&current_line) {
                    if !term.is_empty() && var == term {
                        format.color = egui::Color32::WHITE;
                        format.background = egui::Color32::RED;
                        format.underline = egui::Stroke::new(2.0, egui::Color32::RED);
                    }
                }
                job.append(var, 0.0, format);
                current_line += var.chars().filter(|&c| c == '\n').count();
                char_idx += var.chars().count();
                rest = new_rest;
            } else {
                // Other chars
                let char_len = first_char.len_utf8();
                let (other, new_rest) = rest.split_at(char_len);
                if other == "\n" {
                    current_line += 1;
                } else if other == " " {
                    let in_sel = if let Some((start, end)) = selection {
                        char_idx >= start && char_idx < end
                    } else { false };
                    
                    if in_sel {
                        job.append("·", 0.0, egui::TextFormat::simple(font_id.clone(), egui::Color32::from_gray(100)));
                        char_idx += 1;
                        rest = new_rest;
                        continue;
                    }
                }
                job.append(other, 0.0, egui::TextFormat::simple(font_id.clone(), egui::Color32::WHITE));
                char_idx += 1;
                rest = new_rest;
            }
        } else {
            break;
        }
    }
    job
}

pub fn format_code(code: &str) -> String {
    let mut formatted = String::new();
    let mut depth = 0;
    
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            formatted.push('\n');
            continue;
        }
        
        // Remove comments and string literals for depth counting
        let code_only = if let Some(idx) = trimmed.find('#') {
            &trimmed[..idx]
        } else {
            trimmed
        };
        let code_only = code_only.trim();
        
        if code_only.starts_with('}') && depth > 0 {
            depth -= 1;
        }
        
        formatted.push_str(&"    ".repeat(depth));
        formatted.push_str(trimmed);
        formatted.push('\n');
        
        if code_only.ends_with('{') {
            depth += 1;
        }
    }
    
    formatted
}

