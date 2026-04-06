use serde::{Deserialize, Serialize};
use snenk_bridge_service::{
    preset::{self, SnekPreset},
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vitamins,
    vts::plugin::VTubeStudioPlugin,
};
use std::{
    path::Path,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

slint::include_modules!();

include!(concat!(env!("OUT_DIR"), "/credits.rs"));

// ─── Embedded presets ───────────────────────────────────────────────
// Presets by Maruseu (https://github.com/maruseu/VitaminsPresets),
// included with permission. These are NOT covered by the project's
// GPL license and are NOT republished under GPL.

const PRESET_DEFAULT: &str = include_str!("../presets/default.json");
const PRESET_MARUSEU_VBRIDGER: &str = include_str!("../presets/maruseu_vbridger.json");
const PRESET_MARUSEU_ENHANCED: &str = include_str!("../presets/maruseu_enhanced.json");

const BUILTIN_NAMES: [&str; 3] = ["Default", "Maruseu VBridger", "Maruseu Enhanced"];

// ─── Colors ────────────────────────────────────────────────────────

const COLOR_RED: (u8, u8, u8) = (0xcc, 0x44, 0x44);
const COLOR_YELLOW: (u8, u8, u8) = (0xcc, 0x99, 0x44);
const COLOR_ORANGE: (u8, u8, u8) = (0xdd, 0x77, 0x33);
const COLOR_GREEN: (u8, u8, u8) = (0x5d, 0xba, 0x7d);

fn color(rgb: (u8, u8, u8)) -> slint::Color {
    slint::Color::from_argb_u8(255, rgb.0, rgb.1, rgb.2)
}

// ─── Settings ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default = "default_preset_name")]
    preset_name: String,
    #[serde(default = "default_ip")]
    phone_ip: String,
    #[serde(default)]
    tracking_type_index: i32,
    #[serde(default = "default_timeout")]
    face_search_timeout: String,
    #[serde(default = "default_vts_ip")]
    vts_ip: String,
    #[serde(default = "default_vts_port")]
    vts_port: String,
}

fn default_preset_name() -> String {
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

fn app_dir() -> std::path::PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SnenkBridge");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn settings_path() -> std::path::PathBuf {
    app_dir().join("settings.json")
}

fn presets_dir() -> std::path::PathBuf {
    let dir = app_dir().join("presets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load_settings() -> Settings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

fn save_settings(settings: &Settings) {
    let path = settings_path();
    if let Ok(data) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(&path, data);
    }
}

fn read_settings_from_ui(ui: &App, preset_list: &[PresetEntry]) -> Settings {
    let idx = ui.get_preset_index() as usize;
    let preset_name = preset_list
        .get(idx)
        .map(|e| e.name.clone())
        .unwrap_or_else(default_preset_name);
    Settings {
        preset_name,
        phone_ip: ui.get_phone_ip().to_string(),
        tracking_type_index: ui.get_tracking_type_index(),
        face_search_timeout: ui.get_face_search_timeout().to_string(),
        vts_ip: ui.get_vts_ip().to_string(),
        vts_port: ui.get_vts_port().to_string(),
    }
}

// ─── Preset management ─────────────────────────────────────────────

struct PresetEntry {
    name: String,
    filename: Option<String>, // None for built-ins
}

fn build_preset_list() -> Vec<PresetEntry> {
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

fn refresh_preset_list(ui: &App) -> Vec<PresetEntry> {
    let entries = build_preset_list();
    let names: Vec<slint::SharedString> = entries.iter().map(|e| e.name.clone().into()).collect();
    let model = Rc::new(slint::VecModel::from(names));
    ui.set_preset_names(model.into());
    entries
}

fn resolve_preset(name: &str) -> Result<String, String> {
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

fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

fn tracking_client_type(index: i32) -> TrackingClientType {
    match index {
        1 => TrackingClientType::IFacialMocap,
        _ => TrackingClientType::VTubeStudio,
    }
}

fn timeout_ms(val: &str) -> u64 {
    val.parse::<u64>().unwrap_or(3000)
}

/// Build a SnekPreset for the given preset name (for export).
fn build_snek_preset(name: &str) -> Result<SnekPreset, String> {
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

// ─── Main ──────────────────────────────────────────────────────────

fn main() {
    let log_config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../configs/log_cfg.yml");
    log4rs::init_file(log_config_path, Default::default())
        .expect("Unable to initialize logging from configs/log_cfg.yml");

    let rt = tokio::runtime::Runtime::new().unwrap();

    let app = App::new().unwrap();

    let settings = load_settings();

    // Build preset list and find index of saved preset name
    let entries = refresh_preset_list(&app);
    let saved_index = entries
        .iter()
        .position(|e| e.name == settings.preset_name)
        .unwrap_or(0);

    let preset_list: Arc<Mutex<Vec<PresetEntry>>> = Arc::new(Mutex::new(entries));

    app.set_preset_index(saved_index as i32);
    app.set_can_delete_preset(!is_builtin(&settings.preset_name));
    app.set_phone_ip(settings.phone_ip.into());
    app.set_tracking_type_index(settings.tracking_type_index);
    app.set_face_search_timeout(settings.face_search_timeout.into());
    app.set_vts_ip(settings.vts_ip.into());
    app.set_vts_port(settings.vts_port.into());

    let source_active = Arc::new(AtomicBool::new(false));
    let target_active = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicUsize::new(0));

    // Shared sender for the bridge. Target creates plugin channels and stores
    // the sender here; the source bridge forwards tracking data through it.
    let plugin_tx: Arc<Mutex<Option<Sender<TrackingResponse>>>> = Arc::new(Mutex::new(None));

    // Settings changed -> persist
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = ui.get_preset_index() as usize;
            let is_custom = list
                .get(idx)
                .map(|e| !is_builtin(&e.name))
                .unwrap_or(false);
            ui.set_can_delete_preset(is_custom);
            save_settings(&read_settings_from_ui(&ui, &list));
        });
    }

    // About window
    {
        app.on_show_about(move || {
            let about = AboutWindow::new().unwrap();
            about.set_credits_text(DEPENDENCY_CREDITS.into());
            about.show().unwrap();
        });
    }

    // Import preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_import_preset(move || {
            let weak = weak.clone();
            let preset_list = Arc::clone(&preset_list);
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("All presets", &["snek", "json", "vps"])
                    .add_filter("SnenkBridge preset", &["snek"])
                    .add_filter("Vitamins preset", &["vps"])
                    .add_filter("JSON", &["json"])
                    .pick_file();
                let Some(path) = file else { return };

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(format!("Failed to read file: {e}").into());
                            }
                        });
                        return;
                    }
                };

                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let is_vps = ext == "vps";

                // Pre-parse to extract metadata
                let (title, author, description) = if is_vps {
                    // Try a quick parse for VPS metadata
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(val) => {
                            let t = val
                                .get("saveName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let a = val
                                .get("author")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            (t, a, String::new())
                        }
                        Err(_) => (String::new(), String::new(), String::new()),
                    }
                } else {
                    // .snek or .json
                    match preset::load_from_str(&content) {
                        Ok(p) => (p.title, p.author, p.description),
                        Err(_) => (String::new(), String::new(), String::new()),
                    }
                };

                // Default title from filename if empty
                let title = if title.is_empty() {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Imported")
                        .to_string()
                } else {
                    title
                };

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };

                    let dialog = ImportDialog::new().unwrap();
                    dialog.set_preset_title(title.into());
                    dialog.set_preset_author(author.into());
                    dialog.set_preset_description(description.into());
                    dialog.set_show_swap_toggle(is_vps);
                    dialog.set_swap_xy(false);

                    // do-import
                    {
                        let dialog_weak = dialog.as_weak();
                        let ui_weak = ui.as_weak();
                        let content = content.clone();
                        let preset_list = Arc::clone(&preset_list);
                        dialog.on_do_import(move || {
                            let Some(dlg) = dialog_weak.upgrade() else {
                                return;
                            };
                            let Some(ui) = ui_weak.upgrade() else {
                                return;
                            };

                            let title = dlg.get_preset_title().to_string();
                            let author = dlg.get_preset_author().to_string();
                            let description = dlg.get_preset_description().to_string();
                            let swap_xy = dlg.get_swap_xy();

                            if title.is_empty() {
                                ui.set_error_text("Preset title cannot be empty.".into());
                                return;
                            }

                            // Build the SnekPreset
                            let result: Result<SnekPreset, String> = if is_vps {
                                vitamins::convert_vitamins_to_preset(&content, swap_xy)
                            } else {
                                preset::load_from_str(&content)
                            };

                            match result {
                                Ok(mut snek) => {
                                    snek.title = title.clone();
                                    snek.author = author;
                                    snek.description = description;

                                    let dir = presets_dir();
                                    match preset::save_preset(&dir, &snek) {
                                        Ok(_) => {
                                            let entries = refresh_preset_list(&ui);
                                            let new_idx = entries
                                                .iter()
                                                .position(|e| e.name == title)
                                                .unwrap_or(0);
                                            *preset_list.lock().unwrap() = entries;
                                            ui.set_preset_index(new_idx as i32);
                                            ui.set_can_delete_preset(true);
                                            ui.set_error_text("".into());
                                            save_settings(&read_settings_from_ui(
                                                &ui,
                                                &preset_list.lock().unwrap(),
                                            ));
                                        }
                                        Err(e) => {
                                            ui.set_error_text(
                                                format!("Failed to save preset: {e}").into(),
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    ui.set_error_text(
                                        format!("Failed to parse preset: {e}").into(),
                                    );
                                }
                            }

                            let _ = dlg.hide();
                        });
                    }

                    // do-cancel
                    {
                        let dialog_weak = dialog.as_weak();
                        dialog.on_do_cancel(move || {
                            if let Some(dlg) = dialog_weak.upgrade() {
                                let _ = dlg.hide();
                            }
                        });
                    }

                    dialog.show().unwrap();
                });
            });
        });
    }

    // Export preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_export_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = ui.get_preset_index() as usize;
            let name = list
                .get(idx)
                .map(|e| e.name.clone())
                .unwrap_or_else(default_preset_name);
            drop(list);

            let weak = ui.as_weak();
            std::thread::spawn(move || {
                let snek = match build_snek_preset(&name) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(format!("Export failed: {e}").into());
                            }
                        });
                        return;
                    }
                };

                let default_name = format!("{}.snek", preset::sanitize_title(&snek.title));
                let file = rfd::FileDialog::new()
                    .add_filter("SnenkBridge preset", &["snek"])
                    .set_file_name(&default_name)
                    .save_file();

                if let Some(path) = file {
                    let json = match serde_json::to_string_pretty(&snek) {
                        Ok(j) => j,
                        Err(e) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_error_text(
                                        format!("Failed to serialize preset: {e}").into(),
                                    );
                                }
                            });
                            return;
                        }
                    };
                    if let Err(e) = std::fs::write(&path, json) {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(
                                    format!("Failed to write file: {e}").into(),
                                );
                            }
                        });
                    }
                }
            });
        });
    }

    // Delete preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_delete_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = ui.get_preset_index() as usize;
            let entry = match list.get(idx) {
                Some(e) => e,
                None => return,
            };

            if is_builtin(&entry.name) {
                return;
            }

            let filename = match &entry.filename {
                Some(f) => f.clone(),
                None => return,
            };
            drop(list);

            let dir = presets_dir();
            if let Err(e) = preset::delete_preset(&dir, &filename) {
                ui.set_error_text(format!("Failed to delete preset: {e}").into());
                return;
            }

            let entries = refresh_preset_list(&ui);
            *preset_list.lock().unwrap() = entries;
            ui.set_preset_index(0); // Select Default
            ui.set_can_delete_preset(false);
            ui.set_error_text("".into());
            save_settings(&read_settings_from_ui(&ui, &preset_list.lock().unwrap()));
        });
    }

    // ── Toggle source ──
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let packet_count = Arc::clone(&packet_count);
        let plugin_tx = Arc::clone(&plugin_tx);
        let rt_handle = rt.handle().clone();

        app.on_toggle_source(move || {
            let Some(ui) = weak.upgrade() else { return };

            if source_active.load(Ordering::Relaxed) {
                source_active.store(false, Ordering::Relaxed);
                packet_count.store(0, Ordering::Relaxed);
                ui.set_source_active(false);
                ui.set_source_status("Disconnected".into());
                ui.set_source_status_color(color(COLOR_RED));
                return;
            }

            ui.set_error_text("".into());
            ui.set_source_status("Connecting...".into());
            ui.set_source_status_color(color(COLOR_YELLOW));
            source_active.store(true, Ordering::Relaxed);
            packet_count.store(0, Ordering::Relaxed);
            ui.set_source_active(true);

            let phone_ip = ui.get_phone_ip().to_string();
            let tracking_type = tracking_client_type(ui.get_tracking_type_index());

            let (tracking_tx, tracking_rx) = mpsc::channel::<TrackingResponse>();
            let flag_tracking = Arc::clone(&source_active);
            let pkt_counter = Arc::clone(&packet_count);
            let ptx = Arc::clone(&plugin_tx);
            let source_flag_bridge = Arc::clone(&source_active);

            // Tracking thread
            rt_handle.spawn_blocking(move || {
                let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>) =
                    match tracking_type {
                        TrackingClientType::VTubeStudio => VTubeStudioTrackingClient::run,
                        TrackingClientType::IFacialMocap => IFacialMocapTrackingClinet::run,
                    };
                function(phone_ip, tracking_tx, flag_tracking);
            });

            // Bridge: reads from tracking_rx, forwards to plugin_tx, counts packets
            rt_handle.spawn(async move {
                loop {
                    if !source_flag_bridge.load(Ordering::Relaxed) {
                        break;
                    }
                    match tracking_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(response) => {
                            pkt_counter.fetch_add(1, Ordering::Relaxed);
                            if let Some(ref tx) = *ptx.lock().unwrap() {
                                let _ = tx.send(response);
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            tokio::task::yield_now().await;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        });
    }

    // ── Toggle target ──
    {
        let weak = app.as_weak();
        let target_active = Arc::clone(&target_active);
        let plugin_tx = Arc::clone(&plugin_tx);
        let rt_handle = rt.handle().clone();
        let preset_list = Arc::clone(&preset_list);

        app.on_toggle_target(move || {
            let Some(ui) = weak.upgrade() else { return };

            if target_active.load(Ordering::Relaxed) {
                target_active.store(false, Ordering::Relaxed);
                *plugin_tx.lock().unwrap() = None;
                ui.set_target_active(false);
                ui.set_target_status("Disconnected".into());
                ui.set_target_status_color(color(COLOR_RED));
                return;
            }

            let list = preset_list.lock().unwrap();
            let idx = ui.get_preset_index() as usize;
            let preset_name = list
                .get(idx)
                .map(|e| e.name.clone())
                .unwrap_or_else(default_preset_name);
            drop(list);

            let config_json = match resolve_preset(&preset_name) {
                Ok(json) => json,
                Err(e) => {
                    ui.set_error_text(e.into());
                    return;
                }
            };

            ui.set_error_text("".into());
            ui.set_target_status("Connecting...".into());
            ui.set_target_status_color(color(COLOR_YELLOW));
            target_active.store(true, Ordering::Relaxed);
            ui.set_target_active(true);

            let face_search_timeout = timeout_ms(ui.get_face_search_timeout().as_ref());
            let vts_ip = ui.get_vts_ip().to_string();
            let vts_port = ui.get_vts_port().to_string();

            // Create fresh plugin channels — bridge will pick up the new sender
            let (tx, receiver) = mpsc::channel::<TrackingResponse>();
            *plugin_tx.lock().unwrap() = Some(tx);

            let flag = Arc::clone(&target_active);

            rt_handle.spawn_blocking(move || {
                VTubeStudioPlugin::new(
                    receiver,
                    config_json,
                    face_search_timeout,
                    vts_ip,
                    vts_port,
                )
                .run(flag);
            });
        });
    }

    // ── Status polling ──
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let target_active = Arc::clone(&target_active);
        let packet_count = Arc::clone(&packet_count);

        let src_had_data = Arc::new(AtomicBool::new(false));
        let tgt_ticks = Arc::new(AtomicUsize::new(0));

        rt.spawn(async move {
            let mut last_src_count: usize = 0;
            let mut last_src_time = Instant::now();
            let mut interval = tokio::time::interval(Duration::from_millis(500));

            loop {
                interval.tick().await;

                let src_on = source_active.load(Ordering::Relaxed);
                let tgt_on = target_active.load(Ordering::Relaxed);
                let current_count = packet_count.load(Ordering::Relaxed);
                let now = Instant::now();
                let elapsed = now.duration_since(last_src_time).as_secs_f64();

                let src_rate = if src_on && elapsed > 0.0 && current_count >= last_src_count {
                    (current_count - last_src_count) as f64 / elapsed
                } else {
                    0.0
                };
                last_src_count = current_count;
                last_src_time = now;

                if tgt_on {
                    tgt_ticks.fetch_add(1, Ordering::Relaxed);
                } else {
                    tgt_ticks.store(0, Ordering::Relaxed);
                }

                let weak = weak.clone();
                let src_had = Arc::clone(&src_had_data);
                let tgt_tick_count = tgt_ticks.load(Ordering::Relaxed);

                let ok = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };

                    if src_on {
                        if src_rate > 0.0 {
                            ui.set_source_status(format!("{:.1} packets/s", src_rate).into());
                            ui.set_source_status_color(color(COLOR_GREEN));
                            src_had.store(true, Ordering::Relaxed);
                        } else if src_had.load(Ordering::Relaxed) {
                            ui.set_source_status("Reconnecting...".into());
                            ui.set_source_status_color(color(COLOR_ORANGE));
                        }
                    } else {
                        src_had.store(false, Ordering::Relaxed);
                    }

                    if tgt_on
                        && tgt_tick_count >= 2
                        && ui.get_target_status().as_str() == "Connecting..."
                    {
                        ui.set_target_status("Connected".into());
                        ui.set_target_status_color(color(COLOR_GREEN));
                    }
                });

                if ok.is_err() {
                    break;
                }
            }
        });
    }

    app.run().unwrap();

    // Signal all background tasks to stop on window close
    source_active.store(false, Ordering::Relaxed);
    target_active.store(false, Ordering::Relaxed);

    // Drop the runtime, which waits for blocking tasks to finish
    drop(rt);
}
