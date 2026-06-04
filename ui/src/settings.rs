use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::presets::PresetEntry;
use crate::util::slint_idx;
use crate::App;

#[derive(Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_preset_name")]
    pub preset_name: String,
    #[serde(default = "default_ip")]
    pub phone_ip: String,
    #[serde(default)]
    pub tracking_type_index: i32,
    #[serde(default = "default_timeout")]
    pub face_search_timeout: String,
    #[serde(default = "default_vts_ip")]
    pub vts_ip: String,
    #[serde(default = "default_vts_port")]
    pub vts_port: String,
}

pub fn default_preset_name() -> String {
    "Default".into()
}
fn default_ip() -> String {
    "127.0.0.1".into()
}
fn default_timeout() -> String {
    "3000".into()
}
fn default_vts_ip() -> String {
    "localhost".into()
}
fn default_vts_port() -> String {
    "8001".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preset_name: default_preset_name(),
            phone_ip: default_ip(),
            tracking_type_index: 0,
            face_search_timeout: default_timeout(),
            vts_ip: default_vts_ip(),
            vts_port: default_vts_port(),
        }
    }
}

pub fn app_dir() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SnenkBridge");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn presets_dir() -> PathBuf {
    let dir = app_dir().join("presets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_else(|e| {
            log::warn!("Failed to parse settings, using defaults: {e}");
            Settings::default()
        }),
        Err(_) => Settings::default(), // file not found is normal on first run
    }
}

pub fn save_settings(settings: &Settings) {
    let path = settings_path();
    match serde_json::to_string_pretty(settings) {
        Ok(data) => {
            if let Err(e) = std::fs::write(&path, data) {
                log::warn!("Failed to save settings: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize settings: {e}"),
    }
}

pub fn read_settings_from_ui(ui: &App, preset_list: &[PresetEntry]) -> Settings {
    let idx = slint_idx(ui.get_preset_index());
    let preset_name = preset_list
        .get(idx)
        .map_or_else(default_preset_name, |e| e.name.clone());
    Settings {
        preset_name,
        phone_ip: ui.get_phone_ip().to_string(),
        tracking_type_index: ui.get_tracking_type_index(),
        face_search_timeout: ui.get_face_search_timeout().to_string(),
        vts_ip: ui.get_vts_ip().to_string(),
        vts_port: ui.get_vts_port().to_string(),
    }
}
