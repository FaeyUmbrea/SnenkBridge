use std::rc::Rc;

use snenk_bridge_service::{
    preset::{self, SnekPreset},
    vitamins,
};

use crate::settings::presets_dir;
use crate::App;

// Embedded presets
// Presets by Maruseu (https://github.com/maruseu/VitaminsPresets),
// included with permission. These are NOT covered by the project's
// GPL license and are NOT republished under GPL.

const PRESET_DEFAULT: &str = include_str!("../presets/default.json");
const PRESET_MARUSEU_VBRIDGER: &str = include_str!("../presets/maruseu_vbridger.json");
const PRESET_MARUSEU_ENHANCED: &str = include_str!("../presets/maruseu_enhanced.json");

pub const BUILTIN_NAMES: [&str; 3] = ["Default", "Maruseu VBridger", "Maruseu Enhanced"];

pub struct PresetEntry {
    pub name: String,
    pub filename: Option<String>, // None for built-ins
}

pub fn build_preset_list() -> Vec<PresetEntry> {
    let mut entries: Vec<PresetEntry> = BUILTIN_NAMES
        .iter()
        .map(|n| PresetEntry {
            name: n.to_string(),
            filename: None,
        })
        .collect();
    let custom = preset::list_presets(&presets_dir());
    for p in custom {
        entries.push(PresetEntry {
            name: p.title.clone(),
            filename: Some(format!("{}.snek", preset::sanitize_title(&p.title))),
        });
    }
    entries
}

pub fn refresh_preset_list(ui: &App) -> Vec<PresetEntry> {
    let entries = build_preset_list();
    let names: Vec<slint::SharedString> = entries.iter().map(|e| e.name.clone().into()).collect();
    let model = Rc::new(slint::VecModel::from(names));
    ui.set_preset_names(model.into());
    entries
}

pub fn resolve_preset(name: &str) -> Result<String, String> {
    match name {
        "Default" => Ok(PRESET_DEFAULT.to_string()),
        "Maruseu VBridger" => Ok(PRESET_MARUSEU_VBRIDGER.to_string()),
        "Maruseu Enhanced" => Ok(PRESET_MARUSEU_ENHANCED.to_string()),
        _ => {
            let dir = presets_dir();
            let presets = preset::list_presets(&dir);
            for p in &presets {
                if p.title == name {
                    return serde_json::to_string(&p.params)
                        .map_err(|e| format!("Failed to serialize preset: {e}"));
                }
            }
            Err(format!("Preset not found: {name}"))
        }
    }
}

pub fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

/// Build a `SnekPreset` for the given preset name (for export).
pub fn build_snek_preset(name: &str) -> Result<SnekPreset, String> {
    if is_builtin(name) {
        let json = resolve_preset(name)?;
        let params: Vec<vitamins::CalcFn> =
            serde_json::from_str(&json).map_err(|e| format!("Failed to parse preset: {e}"))?;
        let mut preset = SnekPreset::new(name.to_string(), params);
        if name != "Default" {
            preset.author = "Maruseu".to_string();
        }
        Ok(preset)
    } else {
        let dir = presets_dir();
        let presets = preset::list_presets(&dir);
        for p in presets {
            if p.title == name {
                return Ok(p);
            }
        }
        Err(format!("Preset not found: {name}"))
    }
}
