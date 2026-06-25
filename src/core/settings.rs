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

// Persisted IDE settings.
//
// Preferences (simulation + avrdude), the last opened project and some UI
// layout are saved to a JSON file in the user's config directory and restored
// on the next launch, so nothing has to be reconfigured every session.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// Reopened on the next launch when it still exists.
    pub last_workspace: Option<PathBuf>,

    // Simulation (ik8bvm) preferences.
    pub vm_max_cycles: u32,
    pub sim_trace: bool,
    pub sim_dump_regs: bool,
    pub sim_peek_addr: String,
    pub sim_peek_len: u32,

    // Upload method: bootloader (default) vs avrdude.
    pub use_bootloader: bool,

    // avrdude upload preferences.
    pub avrdude_path: String,
    pub avrdude_programmer: String,
    pub avrdude_port: String,
    pub avrdude_baudrate: String,
    pub avrdude_additional_flags: String,
    pub avrdude_target: String,

    // Serial-bootloader upload preferences.
    pub bootloader_port: String,
    pub bootloader_baud: u32,

    // Burn Bootloader preferences.
    pub burn_path: String,
    #[serde(skip)]
    pub burn_target: String,
    pub burn_f_cpu: u32,
    pub burn_baud: u32,
    pub burn_programmer: String,
    pub burn_port: String,
    pub burn_baudrate: String,
    #[serde(skip)]
    pub burn_additional_flags: String,

    // Serial monitor preferences.
    pub serial_port: String,
    pub serial_baud: u32,

    // UI layout.
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    pub show_terminal: bool,
    pub show_vm_trace: bool,
    pub show_stats: bool,
    pub show_minimap: bool,

    // LLM / AI configuration
    pub llm_provider: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub llm_endpoint: String,
    pub show_ai_chat: bool,
    pub ai_chat_active_tab: String,
    pub agent_cli_command: String,
    /// Agent autonomy over workspace files: "edits" (auto-accept edits),
    /// "yolo" (auto-accept everything) or "readonly".
    pub agent_autonomy: String,
    /// Hide the agent's tool/system activity messages in the chat (cleaner view).
    pub agent_hide_system: bool,
    /// Opt-in: use the agent as the IDE's code assistant (inline suggestions,
    /// helpers). Reserved for the upcoming assistant integration.
    pub agent_as_code_assistant: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_workspace: None,
            vm_max_cycles: 2_000_000,
            sim_trace: true,
            sim_dump_regs: true,
            sim_peek_addr: String::new(),
            sim_peek_len: 1,
            use_bootloader: true,
            avrdude_path: "avrdude".to_string(),
            avrdude_programmer: "usbasp".to_string(),
            avrdude_port: "usb".to_string(),
            avrdude_baudrate: String::new(),
            avrdude_additional_flags: String::new(),
            avrdude_target: "atmega32a".to_string(),
            bootloader_port: String::new(),
            bootloader_baud: 9600,
            burn_path: "avrdude".to_string(),
            burn_target: String::new(),
            burn_f_cpu: 8_000_000,
            burn_baud: 9600,
            burn_programmer: "usbasp".to_string(),
            burn_port: "usb".to_string(),
            burn_baudrate: "-B 10".to_string(),
            burn_additional_flags: String::new(),
            serial_port: String::new(),
            serial_baud: 9600,
            left_panel_width: 250.0,
            right_panel_width: 350.0,
            show_terminal: true,
            show_vm_trace: false,
            show_stats: true,
            show_minimap: true,
 
            // AI agent defaults. `llm_provider` now selects the CLI agent
            // (claude/codex/gemini), driven via the user's own subscription —
            // no API key. The llm_* fields below are retained only for
            // backward-compatible settings round-tripping.
            llm_provider: "claude".to_string(),
            llm_api_key: String::new(),
            llm_model: String::new(),
            llm_endpoint: String::new(),
            show_ai_chat: false,
            ai_chat_active_tab: "simulation".to_string(),
            agent_cli_command: "claude".to_string(),
            agent_autonomy: "edits".to_string(),
            agent_hide_system: true,
            agent_as_code_assistant: false,
        }
    }
}

impl Settings {
    /// `~/.config/ikide/settings.json` (or the platform equivalent).
    pub fn config_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(base.join("ikide").join("settings.json"))
    }

    /// Load saved settings, falling back to defaults when absent or unreadable.
    /// `#[serde(default)]` keeps it forward-compatible when fields are added.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Write the settings to disk (best effort; failures are ignored).
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.avrdude_target = "atmega328p".to_string();
        s.vm_max_cycles = 12345;
        s.last_workspace = Some(PathBuf::from("/tmp/proj"));
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.avrdude_target, "atmega328p");
        assert_eq!(back.vm_max_cycles, 12345);
        assert_eq!(back.last_workspace, Some(PathBuf::from("/tmp/proj")));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // Forward compatibility: an old/partial file still loads.
        let back: Settings = serde_json::from_str("{\"vm_max_cycles\": 7}").unwrap();
        assert_eq!(back.vm_max_cycles, 7);
        assert_eq!(back.avrdude_target, Settings::default().avrdude_target);
        assert!(back.sim_trace); // default true
    }
}
