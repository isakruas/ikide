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

// Small shared UI widgets.

use std::hash::Hash;

use eframe::egui;

/// A dropdown with a built-in text filter, for picking from a long list (MCU
/// targets, virtual devices, components, ...). Returns the value of the item
/// clicked this frame, if any. The filter text is kept in egui's temporary
/// memory keyed by `id_salt`, so callers need no extra state.
pub fn filter_combo<T: Clone>(
    ui: &mut egui::Ui,
    id_salt: impl Hash + Copy,
    selected_text: &str,
    items: &[(T, String)],
) -> Option<T> {
    let filter_id = egui::Id::new("filter_combo_text").with(id_salt);
    let mut filter: String = ui.memory_mut(|m| m.data.get_temp(filter_id).unwrap_or_default());
    let mut chosen = None;

    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("filter…")
                    .desired_width(190.0),
            );
            let q = filter.to_lowercase();
            let mut shown = 0;
            for (value, label) in items {
                if !q.is_empty() && !label.to_lowercase().contains(&q) {
                    continue;
                }
                if ui.selectable_label(false, label).clicked() {
                    chosen = Some(value.clone());
                }
                shown += 1;
            }
            if shown == 0 {
                ui.label(egui::RichText::new("no matches").weak());
            }
        });

    ui.memory_mut(|m| m.data.insert_temp(filter_id, filter));
    chosen
}
