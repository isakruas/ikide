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

//! User-configurable keyboard shortcuts.
//!
//! Each command-level shortcut is a [`Shortcut`] (modifiers + a key name). The
//! whole [`Keymap`] is persisted in settings, so users can view every binding
//! and rebind it without a new build. `command` maps to egui's cross-platform
//! primary modifier (Ctrl on Windows/Linux, Cmd on macOS).

use eframe::egui;
use serde::{Deserialize, Serialize};

/// One keyboard binding: the required modifier state plus a key, stored by its
/// egui name (e.g. "S", "Space", "F5") so it round-trips through JSON.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Shortcut {
    /// Primary modifier: Ctrl on Windows/Linux, Cmd on macOS (egui `command`).
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl Shortcut {
    pub fn new(command: bool, shift: bool, alt: bool, key: &str) -> Self {
        Self { command, shift, alt, key: key.to_string() }
    }

    /// Resolve the stored key name to an egui key.
    pub fn egui_key(&self) -> Option<egui::Key> {
        egui::Key::ALL.iter().copied().find(|k| k.name() == self.key)
    }

    /// True if this shortcut was just pressed, with exactly these modifiers
    /// (extra modifiers don't count, so Shift+R won't fire on Ctrl+Shift+R).
    pub fn pressed(&self, i: &egui::InputState) -> bool {
        let Some(key) = self.egui_key() else { return false };
        i.key_pressed(key)
            && i.modifiers.command == self.command
            && i.modifiers.shift == self.shift
            && i.modifiers.alt == self.alt
    }

    /// Human-readable form for the UI, e.g. "Ctrl+Shift+R".
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" });
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.key.is_empty() {
            "—".to_string()
        } else {
            parts.push(&self.key);
            parts.join("+")
        }
    }
}

/// All configurable command shortcuts. Dialog keys (Enter/Escape) and text
/// editing (Tab/Shift+Tab) are intentionally fixed and not listed here.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Keymap {
    pub save: Shortcut,
    pub ai_edit: Shortcut,
    pub compile: Shortcut,
    pub simulate: Shortcut,
    pub autocomplete: Shortcut,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            save: Shortcut::new(true, false, false, "S"),
            ai_edit: Shortcut::new(true, false, false, "I"),
            compile: Shortcut::new(false, true, false, "R"),
            simulate: Shortcut::new(false, true, false, "S"),
            autocomplete: Shortcut::new(true, false, false, "Space"),
        }
    }
}

impl Keymap {
    /// (label, binding) for every action, for listing/rebinding in the UI.
    pub fn entries(&mut self) -> [(&'static str, &mut Shortcut); 5] {
        [
            ("Save file", &mut self.save),
            ("AI Edit (code assistant)", &mut self.ai_edit),
            ("Compile", &mut self.compile),
            ("Simulate", &mut self.simulate),
            ("Trigger autocomplete", &mut self.autocomplete),
        ]
    }
}

/// The non-modifier key from the latest key-press this frame, plus the modifier
/// state, as an egui name. Used to capture a new binding while rebinding.
pub fn captured_press(i: &egui::InputState) -> Option<Shortcut> {
    for ev in &i.events {
        if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
            return Some(Shortcut {
                command: modifiers.command,
                shift: modifiers.shift,
                alt: modifiers.alt,
                key: key.name().to_string(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_labels() {
        let s = Shortcut::new(false, true, false, "R");
        let json = serde_json::to_string(&s).unwrap();
        let back: Shortcut = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(back.label().ends_with("Shift+R"));
    }

    #[test]
    fn defaults_resolve_to_keys() {
        let km = Keymap::default();
        assert!(km.save.egui_key().is_some());
        assert!(km.autocomplete.egui_key().is_some());
    }
}
