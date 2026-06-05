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

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use eframe::egui;

use crate::core::analysis::{self, Diagnostic, SymbolIndex};
use crate::core::runner::{self, TaskMsg};
use crate::core::workspace::{self, FileNode};

/// How long the buffer must be idle before we re-run the background type-check.
const CHECK_DEBOUNCE: Duration = Duration::from_millis(500);
use crate::ui::{editor, explorer, right_panel, terminal};

pub struct OpenTab {
    pub path: PathBuf,
    pub content: String,
    pub is_modified: bool,
}

pub enum ActionPopup {
    None,
    Rename { path: PathBuf, new_name: String },
    CreateFile { parent_dir: PathBuf, new_name: String },
    CreateDir { parent_dir: PathBuf, new_name: String },
    Delete { path: PathBuf },
}

pub struct IkIdeApp {
    pub workspace_dir: Option<PathBuf>,
    pub workspace_tree: Option<FileNode>,
    
    // Tab management
    pub open_tabs: Vec<OpenTab>,
    pub active_tab: Option<usize>,
    
    pub terminal_output: String,
    pub vm_output: String,
    pub stats_data: Result<Option<crate::core::runner::StatsData>, String>,
    
    pub task_rx: Receiver<TaskMsg>,
    pub task_tx: Sender<TaskMsg>,
    
    pub dialog_rx: Option<Receiver<Option<PathBuf>>>,
    
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    
    pub show_terminal: bool,
    pub show_vm_trace: bool,
    pub show_stats: bool,
    pub show_minimap: bool,
    pub show_preferences: bool,
    pub is_busy: bool,
    
    pub compiler_path: String,
    pub vm_path: String,
    pub vm_max_cycles: u32,

    // Avrdude upload preferences
    pub avrdude_path: String,
    pub avrdude_programmer: String,
    pub avrdude_port: String,
    pub avrdude_baudrate: String,
    pub avrdude_additional_flags: String,
    pub avrdude_target: String,

    // Language intelligence (compiler-backed).
    pub std_index: SymbolIndex,
    pub devices: Vec<(String, String)>,
    pub diagnostics: Vec<Diagnostic>,
    pub last_edit: Option<Instant>,
    pub last_checked_content: String,

    pub action_popup: ActionPopup,
}

impl Default for IkIdeApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let compiler_path = std::fs::canonicalize("tools/ik8b")
            .unwrap_or_else(|_| PathBuf::from("tools/ik8b"))
            .to_string_lossy()
            .into_owned();
        let std_dir = analysis::std_dir_from_compiler(&compiler_path);
        // Let the in-process compiler front end resolve `import`s against std.
        // (env::set_var is `unsafe` on edition 2024.)
        unsafe { std::env::set_var("IK8B_STD_PATH", &std_dir); }
        // Index the std library once at startup (cheap line scan).
        let std_index = SymbolIndex::from_dir(&std_dir);
        // Supported target chips, straight from the compiler's device table.
        let devices = analysis::load_devices();

        let app = Self {
            workspace_dir: None,
            workspace_tree: None,
            open_tabs: Vec::new(),
            active_tab: None,
            terminal_output: "Welcome to IKIDE!\n".to_string(),
            vm_output: String::new(),
            stats_data: Ok(None),
            task_rx: rx,
            task_tx: tx,
            dialog_rx: None,
            left_panel_width: 250.0,
            right_panel_width: 350.0,
            show_terminal: true,
            show_vm_trace: false,
            show_stats: true,
            show_minimap: true,
            show_preferences: false,
            is_busy: false,
            compiler_path,
            vm_path: std::fs::canonicalize("tools/avr-vm/bin/avr_vm").unwrap_or_else(|_| PathBuf::from("tools/avr-vm/bin/avr_vm")).to_string_lossy().into_owned(),
            vm_max_cycles: 2000000,
            avrdude_path: "avrdude".to_string(),
            avrdude_programmer: "usbasp".to_string(),
            avrdude_port: "usb".to_string(),
            avrdude_baudrate: "".to_string(),
            avrdude_additional_flags: "".to_string(),
            avrdude_target: "atmega32a".to_string(),
            std_index,
            devices,
            diagnostics: Vec::new(),
            last_edit: None,
            last_checked_content: String::new(),
            action_popup: ActionPopup::None,
        };

        app
    }
}

impl IkIdeApp {
    pub fn open_folder_dialog(&mut self) {
        if self.dialog_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.dialog_rx = Some(rx);
            std::thread::spawn(move || {
                let folder = rfd::FileDialog::new().pick_folder();
                let _ = tx.send(folder);
            });
        }
    }

    /// All diagnostics to display: the compiler's (debounced) plus the IDE's
    /// instant lints on the active buffer. When both flag the same line, the
    /// precise lint wins so the squiggle lands on the exact token.
    pub fn all_diagnostics(&self) -> Vec<Diagnostic> {
        let lints = match self.active_tab.and_then(|i| self.open_tabs.get(i)) {
            Some(tab) => analysis::lint_buffer(&tab.content),
            None => Vec::new(),
        };
        let lint_lines: std::collections::HashSet<usize> = lints.iter().map(|d| d.line).collect();

        let mut out: Vec<Diagnostic> = self
            .diagnostics
            .iter()
            .filter(|d| d.line == 0 || !lint_lines.contains(&d.line))
            .cloned()
            .collect();
        out.extend(lints);
        out.sort_by_key(|d| d.line);
        out
    }

    /// Debounced live type-check: when the active buffer has been idle for
    /// `CHECK_DEBOUNCE`, run the compiler's front end in-process and refresh the
    /// diagnostics. Returns how long until the next check might fire, so the
    /// caller can schedule a repaint (egui is event-driven).
    pub fn maybe_run_check(&mut self) -> Option<Duration> {
        let idx = self.active_tab?;
        let content = self.open_tabs.get(idx)?.content.clone();

        if content == self.last_checked_content {
            return None;
        }
        match self.last_edit {
            Some(t) if t.elapsed() >= CHECK_DEBOUNCE => {
                // Fast and synchronous: it's the real lexer + parser, in-process.
                self.diagnostics = analysis::check(&content);
                self.last_checked_content = content;
                None
            }
            Some(t) => Some(CHECK_DEBOUNCE.saturating_sub(t.elapsed())),
            None => Some(CHECK_DEBOUNCE),
        }
    }

    pub fn refresh_files(&mut self) {
        if let Some(dir) = &self.workspace_dir {
            let mut tree = workspace::scan_workspace(dir);
            tree.name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.workspace_tree = Some(tree);
        }
    }

    pub fn load_file(&mut self, path: PathBuf) {
        if let Some(idx) = self.open_tabs.iter().position(|t| t.path == path) {
            self.active_tab = Some(idx);
            return;
        }
        
        if let Ok(content) = std::fs::read_to_string(&path) {
            self.open_tabs.push(OpenTab {
                path,
                content,
                is_modified: false,
            });
            self.active_tab = Some(self.open_tabs.len() - 1);
        } else {
            self.terminal_output.push_str(&format!("Failed to load file: {:?}\n", path));
            self.show_terminal = true;
        }
    }

    pub fn save_active_file(&mut self) {
        if let Some(idx) = self.active_tab {
            let tab = &mut self.open_tabs[idx];
            // Auto-format on save so the on-disk file is always tidy.
            tab.content = crate::syntax::format_code(&tab.content);
            if let Err(e) = std::fs::write(&tab.path, &tab.content) {
                self.terminal_output.push_str(&format!("Failed to save file: {}\n", e));
                self.show_terminal = true;
            } else {
                tab.is_modified = false;
                self.terminal_output.push_str(&format!("Saved {:?}\n", tab.path));
                // Trigger stats update on save (in-process build).
                runner::spawn_stats(tab.content.clone(), self.task_tx.clone());
            }
        }
    }

    pub fn handle_background_tasks(&mut self) {
        while let Ok(msg) = self.task_rx.try_recv() {
            match msg {
                TaskMsg::Done => {
                    self.is_busy = false;
                    self.refresh_files();
                }
                TaskMsg::Compile(out) => {
                    self.terminal_output.push_str(&out);
                }
                TaskMsg::Vm(out) => {
                    self.vm_output.push_str(&out);
                }
                TaskMsg::Upload(out) => {
                    self.terminal_output.push_str(&out);
                    self.show_terminal = true;
                }
                TaskMsg::Stats(res) => {
                    self.stats_data = match res {
                        Ok(data) => Ok(Some(data)),
                        Err(e) => Err(e),
                    };
                }
            }
        }
    }
    
    pub fn render_popups(&mut self, ctx: &egui::Context) {
        let mut close_popup = false;
        
        match &mut self.action_popup {
            ActionPopup::None => {}
            ActionPopup::Rename { path, new_name } => {
                egui::Window::new("Rename").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("✏ Rename").strong());
                    ui.separator();
                    let path_clone = path.clone();
                    let path_clone2 = path.clone();
                    ui.horizontal(|ui| {
                        ui.label("New name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let mut new_path = path_clone.clone();
                            new_path.set_file_name(&*new_name);
                            if std::fs::rename(&path_clone, new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let mut new_path = path_clone2.clone();
                            new_path.set_file_name(&*new_name);
                            if std::fs::rename(&path_clone2, new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::CreateFile { parent_dir, new_name } => {
                egui::Window::new("Create File").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("📄 Create File").strong());
                    ui.separator();
                    let dir_clone = parent_dir.clone();
                    let dir_clone2 = parent_dir.clone();
                    ui.horizontal(|ui| {
                        ui.label("File name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let new_path = dir_clone.join(&*new_name);
                            if std::fs::write(&new_path, "").is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let new_path = dir_clone2.join(&*new_name);
                            if std::fs::write(&new_path, "").is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::CreateDir { parent_dir, new_name } => {
                egui::Window::new("Create Directory").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("📁 Create Directory").strong());
                    ui.separator();
                    let dir_clone = parent_dir.clone();
                    let dir_clone2 = parent_dir.clone();
                    ui.horizontal(|ui| {
                        ui.label("Directory name:");
                        let response = ui.text_edit_singleline(new_name);
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let new_path = dir_clone.join(&*new_name);
                            if std::fs::create_dir(&new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Confirm").clicked() {
                            let new_path = dir_clone2.join(&*new_name);
                            if std::fs::create_dir(&new_path).is_ok() {
                                close_popup = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
            ActionPopup::Delete { path } => {
                egui::Window::new("Delete").title_bar(false).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(egui::RichText::new("🗑 Confirm Delete").strong());
                    ui.separator();
                    ui.label(format!("Are you sure you want to delete {:?}?", path.file_name().unwrap_or_default()));
                    let path_clone = path.clone();
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            if path_clone.is_dir() {
                                let _ = std::fs::remove_dir_all(&path_clone);
                            } else {
                                let _ = std::fs::remove_file(&path_clone);
                            }
                            close_popup = true;
                        }
                        if ui.button("No").clicked() {
                            close_popup = true;
                        }
                    });
                });
            }
        }
        
        if close_popup {
            self.action_popup = ActionPopup::None;
            self.refresh_files();
        }
    }
}

impl eframe::App for IkIdeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keyboard shortcuts
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_active_file();
        }

        self.handle_background_tasks();

        // Live, debounced background type-check of the active buffer.
        if let Some(wait) = self.maybe_run_check() {
            ctx.request_repaint_after(wait);
        }

        // Check shortcuts
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::R)) {
            if !self.is_busy && self.active_tab.is_some() {
                self.save_active_file();
                self.is_busy = true;
                self.show_terminal = true;
                self.terminal_output.clear();
                self.terminal_output.push_str("--- Compiling ---\n");
                let (path, content) = self.active_tab
                    .map(|idx| (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone()))
                    .unwrap_or((None, String::new()));
                runner::spawn_compile(self.workspace_dir.clone(), path, content, self.task_tx.clone());
            }
        }
        
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::S)) {
            if !self.is_busy && self.active_tab.is_some() {
                self.is_busy = true;
                self.show_vm_trace = true;
                self.vm_output.clear();
                self.vm_output.push_str("--- VM Trace Starting ---\n");
                let (path, text) = if let Some(idx) = self.active_tab {
                    (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone())
                } else {
                    (None, String::new())
                };
                runner::spawn_simulate(self.workspace_dir.clone(), path, text, self.task_tx.clone(), self.vm_path.clone(), self.vm_max_cycles);
            }
        }

        // Handle incoming messagesult
        if let Some(rx) = &self.dialog_rx {
            if let Ok(folder_opt) = rx.try_recv() {
                self.dialog_rx = None;
                if let Some(folder) = folder_opt {
                    self.workspace_dir = Some(folder);
                    self.open_tabs.clear();
                    self.active_tab = None;
                    self.refresh_files();
                }
            }
        }
        
        self.render_popups(ctx);

        if self.is_busy || self.dialog_rx.is_some() {
            ctx.request_repaint();
        }

        if self.workspace_dir.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("Welcome to IKIDE");
                    ui.add_space(20.0);
                    if ui.add_enabled(self.dialog_rx.is_none(), egui::Button::new("📂 Open Project Folder")).clicked() {
                        self.open_folder_dialog();
                    }
                    if self.dialog_rx.is_some() {
                        ui.add_space(10.0);
                        ui.spinner();
                        ui.label("Waiting for folder selection...");
                    }
                });
            });
            return;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.add_enabled(self.dialog_rx.is_none(), egui::Button::new("📂 Open Project")).clicked() {
                        self.open_folder_dialog();
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("💾 Save (Ctrl+S)")).clicked() {
                        self.save_active_file();
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("✨ Format Code")).clicked() {
                        if let Some(idx) = self.active_tab {
                            let unformatted = self.open_tabs[idx].content.clone();
                            self.open_tabs[idx].content = crate::syntax::format_code(&unformatted);
                            self.open_tabs[idx].is_modified = true;
                        }
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_terminal, "Output");
                    ui.checkbox(&mut self.show_vm_trace, "VM Trace");
                    ui.checkbox(&mut self.show_stats, "Resource Stats");
                    ui.checkbox(&mut self.show_minimap, "Minimap");
                    ui.separator();
                    if ui.add_enabled(!self.is_busy, egui::Button::new("🔄 Refresh Explorer")).clicked() {
                        self.refresh_files();
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Run", |ui| {
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("🔨 Compile (Shift+R)")).clicked() {
                        self.save_active_file();
                        self.is_busy = true;
                        self.show_terminal = true;
                        self.terminal_output.clear();
                        self.terminal_output.push_str("--- Compiling ---\n");
                        let (path, content) = self.active_tab
                            .map(|idx| (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone()))
                            .unwrap_or((None, String::new()));
                        runner::spawn_compile(self.workspace_dir.clone(), path, content, self.task_tx.clone());
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("🚀 Simulate (Shift+S)")).clicked() {
                        self.is_busy = true;
                        self.show_vm_trace = true;
                        self.vm_output.clear();
                        self.vm_output.push_str("--- VM Trace Starting ---\n");
                        let (path, text) = if let Some(idx) = self.active_tab {
                            (Some(self.open_tabs[idx].path.clone()), self.open_tabs[idx].content.clone())
                        } else { (None, String::new()) };
                        runner::spawn_simulate(self.workspace_dir.clone(), path, text, self.task_tx.clone(), self.vm_path.clone(), self.vm_max_cycles);
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.is_busy && self.active_tab.is_some(), egui::Button::new("🔌 Upload to Board")).clicked() {
                        self.save_active_file();
                        self.is_busy = true;
                        self.show_terminal = true;
                        self.terminal_output.clear();
                        self.terminal_output.push_str("--- Uploading ---\n");
                        let path = self.active_tab.map(|idx| self.open_tabs[idx].path.clone());
                        runner::spawn_upload(self.workspace_dir.clone(), path, self.avrdude_path.clone(), self.avrdude_target.clone(), self.avrdude_programmer.clone(), self.avrdude_port.clone(), self.avrdude_baudrate.clone(), self.avrdude_additional_flags.clone(), self.task_tx.clone());
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Config", |ui| {
                    if ui.button("Preferences...").clicked() {
                        self.show_preferences = true;
                        ui.close_menu();
                    }
                });
                
                if self.is_busy {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label("Working...");
                        ui.spinner();
                    });
                }
            });
        });

        if self.show_preferences {
            let mut show = self.show_preferences;
            let mut close = false;
            egui::Window::new("Preferences")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .open(&mut show)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("⚙ Preferences").strong());
                    ui.separator();
                    egui::Grid::new("prefs_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                        ui.label("Compiler Path:");
                        ui.text_edit_singleline(&mut self.compiler_path);
                        ui.end_row();
                        
                        ui.label("VM Path:");
                        ui.text_edit_singleline(&mut self.vm_path);
                        ui.end_row();
                        
                        ui.label("VM Max Cycles:");
                        ui.add(egui::DragValue::new(&mut self.vm_max_cycles).speed(1000).range(1..=100_000_000));
                        ui.end_row();
                    });
                    
                    ui.add_space(5.0);
                    
                    egui::CollapsingHeader::new("Avrdude Configuration").default_open(true).show(ui, |ui| {
                        egui::Grid::new("avrdude_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                            ui.label("Executable Path:");
                            ui.text_edit_singleline(&mut self.avrdude_path);
                            ui.end_row();

                            ui.label("Target MCU:");
                            egui::ComboBox::from_id_salt("mcu_target")
                                .selected_text(&self.avrdude_target)
                                .show_ui(ui, |ui| {
                                    for (dev, _) in &self.devices {
                                        ui.selectable_value(&mut self.avrdude_target, dev.clone(), dev);
                                    }
                                });
                            ui.end_row();
                            
                            ui.label("Programmer:");
                            egui::ComboBox::from_id_salt("programmer_target")
                                .selected_text(&self.avrdude_programmer)
                                .show_ui(ui, |ui| {
                                    for p in ["arduino", "usbasp", "usbtiny", "avrispmkII", "stk500v1", "stk500v2", "micronucleus"] {
                                        ui.selectable_value(&mut self.avrdude_programmer, p.to_string(), p);
                                    }
                                });
                            ui.end_row();
                            
                            ui.label("Port:");
                            ui.text_edit_singleline(&mut self.avrdude_port);
                            ui.end_row();

                            ui.label("Baudrate (opt):");
                            ui.text_edit_singleline(&mut self.avrdude_baudrate);
                            ui.end_row();
                            
                            ui.label("Additional Flags:");
                            ui.text_edit_singleline(&mut self.avrdude_additional_flags);
                            ui.end_row();
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save & Close").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                show = false;
            }
            self.show_preferences = show;
        }

        if self.show_terminal {
            terminal::render(self, ctx);
        }
        explorer::render(self, ctx);
        if self.show_vm_trace {
            right_panel::render(self, ctx);
        }
        editor::render(self, ctx);
    }
}
