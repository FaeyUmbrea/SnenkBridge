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
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

slint::include_modules!();

// ─── Settings persistence ──────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    transform_path: String,
    ip: String,
    tracking_type_index: i32,
    face_search_timeout: String,
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

// ─── Helpers ───────────────────────────────────────────────────────

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

    let app = App::new().unwrap();

    // Load persisted settings
    let settings = load_settings();
    app.set_transform_path(settings.transform_path.into());
    app.set_ip(if settings.ip.is_empty() {
        "127.0.0.1".into()
    } else {
        settings.ip.into()
    });
    app.set_tracking_type_index(settings.tracking_type_index);
    app.set_face_search_timeout(if settings.face_search_timeout.is_empty() {
        "3000".into()
    } else {
        settings.face_search_timeout.into()
    });

    // Shared state
    let active = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicUsize::new(0));

    // ── Settings changed callback ──
    {
        let weak = app.as_weak();
        app.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            let s = Settings {
                transform_path: ui.get_transform_path().to_string(),
                ip: ui.get_ip().to_string(),
                tracking_type_index: ui.get_tracking_type_index(),
                face_search_timeout: ui.get_face_search_timeout().to_string(),
            };
            save_settings(&s);
        });
    }

    // ── Browse file callback ──
    {
        let weak = app.as_weak();
        app.on_browse_file(move || {
            let weak = weak.clone();
            thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("Config files", &["json", "vps"])
                    .add_filter("JSON", &["json"])
                    .add_filter("Vitamins preset", &["vps"])
                    .pick_file();
                if let Some(path) = file {
                    let path_str: slint::SharedString = path.to_string_lossy().to_string().into();
                    let weak = weak.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_transform_path(path_str);
                            // Save after browse
                            let s = Settings {
                                transform_path: ui.get_transform_path().to_string(),
                                ip: ui.get_ip().to_string(),
                                tracking_type_index: ui.get_tracking_type_index(),
                                face_search_timeout: ui.get_face_search_timeout().to_string(),
                            };
                            save_settings(&s);
                        }
                    })
                    .ok();
                }
            });
        });
    }

    // ── Toggle connection callback ──
    {
        let weak = app.as_weak();
        let active = Arc::clone(&active);
        let packet_count = Arc::clone(&packet_count);

        app.on_toggle_connection(move || {
            let Some(ui) = weak.upgrade() else { return };

            if active.load(Ordering::Relaxed) {
                // Disconnect
                active.store(false, Ordering::Relaxed);
                packet_count.store(0, Ordering::Relaxed);
                ui.set_active(false);
                ui.set_status_text("Disconnected".into());
                ui.set_status_color(slint::Color::from_argb_u8(255, 0x6a, 0x6a, 0x8a));
                ui.set_error_text("".into());
            } else {
                // Validate config
                let transform_path = ui.get_transform_path().to_string();
                let config_path = Path::new(&transform_path);
                if !config_path.is_file() {
                    ui.set_error_text(
                        format!("Config file not found: {}", transform_path).into(),
                    );
                    return;
                }

                ui.set_error_text("".into());
                active.store(true, Ordering::Relaxed);
                packet_count.store(0, Ordering::Relaxed);
                ui.set_active(true);

                let ip = ui.get_ip().to_string();
                let face_search_timeout = timeout_ms(&ui.get_face_search_timeout().to_string());
                let tracking_type = tracking_client_type(ui.get_tracking_type_index());

                let (tracking_sender, tracking_receiver): (
                    Sender<TrackingResponse>,
                    Receiver<TrackingResponse>,
                ) = mpsc::channel();
                let (plugin_sender, plugin_receiver): (
                    Sender<TrackingResponse>,
                    Receiver<TrackingResponse>,
                ) = mpsc::channel();

                let flag_plugin = Arc::clone(&active);
                let flag_tracking = Arc::clone(&active);
                let pkt_counter = Arc::clone(&packet_count);
                let active_bridge = Arc::clone(&active);

                // Bridge thread
                thread::spawn(move || {
                    while active_bridge.load(Ordering::Relaxed) {
                        match tracking_receiver.recv_timeout(Duration::from_millis(200)) {
                            Ok(response) => {
                                pkt_counter.fetch_add(1, Ordering::Relaxed);
                                if plugin_sender.send(response).is_err() {
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                });

                // Plugin thread
                let path = transform_path.clone();
                thread::spawn(move || {
                    VTubeStudioPlugin::new(plugin_receiver, path, 0, face_search_timeout).run(flag_plugin);
                });

                // Tracking thread
                let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>);
                match tracking_type {
                    TrackingClientType::VTubeStudio => function = VTubeStudioTrackingClient::run,
                    TrackingClientType::IFacialMocap => function = IFacialMocapTrackingClinet::run,
                }
                thread::spawn(move || function(ip, tracking_sender, flag_tracking));
            }
        });
    }

    // ── Packet rate polling timer ──
    {
        let weak = app.as_weak();
        let active = Arc::clone(&active);
        let packet_count = Arc::clone(&packet_count);

        thread::spawn(move || {
            let mut last_count: usize = 0;
            let mut last_time = Instant::now();

            loop {
                thread::sleep(Duration::from_millis(500));

                let is_active = active.load(Ordering::Relaxed);
                let current_count = packet_count.load(Ordering::Relaxed);
                let now = Instant::now();
                let elapsed = now.duration_since(last_time).as_secs_f64();

                let rate = if is_active && elapsed > 0.0 && current_count >= last_count {
                    (current_count - last_count) as f64 / elapsed
                } else {
                    0.0
                };

                last_count = current_count;
                last_time = now;

                let weak = weak.clone();
                let ok = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        if ui.get_active() {
                            ui.set_status_text(format!("{:.1} packets/s", rate).into());
                            ui.set_status_color(slint::Color::from_argb_u8(255, 0x5d, 0xba, 0x7d));
                        }
                    }
                });

                if ok.is_err() {
                    break; // Event loop closed
                }
            }
        });
    }

    app.run().unwrap();
}
