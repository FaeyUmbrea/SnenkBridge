use serde::{Deserialize, Serialize};
use snenk_bridge_service::{
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vts::plugin::VTubeStudioPlugin,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

slint::include_modules!();

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
    #[serde(default)]
    transform_path: String,
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
            transform_path: String::new(),
            phone_ip: default_ip(),
            tracking_type_index: 0,
            face_search_timeout: default_timeout(),
            vts_ip: default_vts_ip(),
            vts_port: default_vts_port(),
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    if let Some(dir) = dirs::config_dir() {
        let app_dir = dir.join("SnenkBridge");
        let _ = std::fs::create_dir_all(&app_dir);
        app_dir.join("settings.json")
    } else {
        std::path::PathBuf::from("settings.json")
    }
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

fn read_settings_from_ui(ui: &App) -> Settings {
    Settings {
        transform_path: ui.get_transform_path().to_string(),
        phone_ip: ui.get_phone_ip().to_string(),
        tracking_type_index: ui.get_tracking_type_index(),
        face_search_timeout: ui.get_face_search_timeout().to_string(),
        vts_ip: ui.get_vts_ip().to_string(),
        vts_port: ui.get_vts_port().to_string(),
    }
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

// ─── Main ──────────────────────────────────────────────────────────

fn main() {
    let log_config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../configs/log_cfg.yml");
    log4rs::init_file(log_config_path, Default::default())
        .expect("Unable to initialize logging from configs/log_cfg.yml");

    let rt = tokio::runtime::Runtime::new().unwrap();

    let app = App::new().unwrap();

    let settings = load_settings();
    app.set_transform_path(settings.transform_path.into());
    app.set_phone_ip(settings.phone_ip.into());
    app.set_tracking_type_index(settings.tracking_type_index);
    app.set_face_search_timeout(settings.face_search_timeout.into());
    app.set_vts_ip(settings.vts_ip.into());
    app.set_vts_port(settings.vts_port.into());

    let source_active = Arc::new(AtomicBool::new(false));
    let target_active = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicUsize::new(0));

    // Shared channel endpoints for the bridge between source and target.
    // Source writes TrackingResponse into plugin_tx; target reads from plugin_rx.
    // These are wrapped in Arc<Mutex<Option<...>>> so they can be swapped out
    // each time source/target reconnects.
    let plugin_tx: Arc<Mutex<Option<Sender<TrackingResponse>>>> = Arc::new(Mutex::new(None));
    let plugin_rx: Arc<Mutex<Option<std::sync::mpsc::Receiver<TrackingResponse>>>> =
        Arc::new(Mutex::new(None));

    // Settings changed → persist
    {
        let weak = app.as_weak();
        app.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            save_settings(&read_settings_from_ui(&ui));
        });
    }

    // Browse file
    {
        let weak = app.as_weak();
        app.on_browse_file(move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("Config files", &["json", "vps"])
                    .add_filter("JSON", &["json"])
                    .add_filter("Vitamins preset", &["vps"])
                    .pick_file();
                if let Some(path) = file {
                    let path_str: slint::SharedString = path.to_string_lossy().to_string().into();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_transform_path(path_str);
                            save_settings(&read_settings_from_ui(&ui));
                        }
                    });
                }
            });
        });
    }

    // ── Toggle source ──
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let packet_count = Arc::clone(&packet_count);
        let plugin_tx = Arc::clone(&plugin_tx);
        let plugin_rx = Arc::clone(&plugin_rx);
        let rt_handle = rt.handle().clone();

        app.on_toggle_source(move || {
            let Some(ui) = weak.upgrade() else { return };

            if source_active.load(Ordering::Relaxed) {
                source_active.store(false, Ordering::Relaxed);
                packet_count.store(0, Ordering::Relaxed);
                // Drop the old plugin channel so target knows source stopped
                *plugin_tx.lock().unwrap() = None;
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

            // Create fresh channels
            let (tx, rx) = mpsc::channel::<TrackingResponse>();
            *plugin_tx.lock().unwrap() = Some(tx);
            *plugin_rx.lock().unwrap() = Some(rx);

            let (tracking_tx, tracking_rx) = mpsc::channel::<TrackingResponse>();
            let flag_tracking = Arc::clone(&source_active);
            let pkt_counter = Arc::clone(&packet_count);
            let ptx = Arc::clone(&plugin_tx);
            let source_flag_bridge = Arc::clone(&source_active);

            // Tracking thread
            rt_handle.spawn_blocking(move || {
                let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>) = match tracking_type {
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
        let plugin_rx = Arc::clone(&plugin_rx);
        let rt_handle = rt.handle().clone();

        app.on_toggle_target(move || {
            let Some(ui) = weak.upgrade() else { return };

            if target_active.load(Ordering::Relaxed) {
                target_active.store(false, Ordering::Relaxed);
                ui.set_target_active(false);
                ui.set_target_status("Disconnected".into());
                ui.set_target_status_color(color(COLOR_RED));
                return;
            }

            let transform_path = ui.get_transform_path().to_string();
            if !Path::new(&transform_path).is_file() {
                ui.set_error_text(format!("Config file not found: {}", transform_path).into());
                return;
            }

            ui.set_error_text("".into());
            ui.set_target_status("Connecting...".into());
            ui.set_target_status_color(color(COLOR_YELLOW));
            target_active.store(true, Ordering::Relaxed);
            ui.set_target_active(true);

            let face_search_timeout = timeout_ms(ui.get_face_search_timeout().as_ref());
            let vts_ip = ui.get_vts_ip().to_string();
            let vts_port = ui.get_vts_port().to_string();

            // Take the plugin_rx if source already created one; otherwise create a dummy channel
            let rx = plugin_rx.lock().unwrap().take();
            let receiver = rx.unwrap_or_else(|| {
                let (_tx, rx) = mpsc::channel();
                rx
            });

            let flag = Arc::clone(&target_active);

            rt_handle.spawn_blocking(move || {
                VTubeStudioPlugin::new(
                    receiver,
                    transform_path,
                    0,
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
}
