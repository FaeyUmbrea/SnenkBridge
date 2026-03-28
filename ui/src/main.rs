use eframe::egui;
use log4rs;
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

fn main() {
    let log_config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../configs/log_cfg.yml");
    log4rs::init_file(log_config_path, Default::default())
        .expect("Unable to initialize logging from configs/log_cfg.yml");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([300.0, 180.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SnenkBridge",
        native_options,
        Box::new(|cc| Ok(Box::new(SnenkBridgeUI::new(cc)))),
    )
    .unwrap();
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct SnenkBridgeUI {
    transform_path: String,
    ip: String,
    tracking_client_type: TrackingClientType,
    face_search_timeout: i64,

    #[serde(skip)]
    active: Arc<AtomicBool>,
    #[serde(skip)]
    packet_count: Arc<AtomicUsize>,
    #[serde(skip)]
    config_error: Option<String>,
    #[serde(skip)]
    last_packet_count: usize,
    #[serde(skip)]
    last_packet_time: Instant,
    #[serde(skip)]
    packet_rate: f64,
    #[serde(skip)]
    file_dialog_receiver: Option<Receiver<Option<std::path::PathBuf>>>,
}

impl Default for SnenkBridgeUI {
    fn default() -> Self {
        Self {
            transform_path: String::new(),
            ip: "127.0.0.1".to_string(),
            tracking_client_type: TrackingClientType::VTubeStudio,
            face_search_timeout: 3000,
            active: Arc::new(AtomicBool::new(false)),
            packet_count: Arc::new(AtomicUsize::new(0)),
            config_error: None,
            last_packet_count: 0,
            last_packet_time: Instant::now(),
            packet_rate: 0.0,
            file_dialog_receiver: None,
        }
    }
}

impl SnenkBridgeUI {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut ui: Self = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        ui.active = Arc::new(AtomicBool::new(false));
        ui.packet_count = Arc::new(AtomicUsize::new(0));
        ui.config_error = None;
        ui.last_packet_count = 0;
        ui.last_packet_time = Instant::now();
        ui.packet_rate = 0.0;
        ui.file_dialog_receiver = None;
        ui
    }

    fn open_file_dialog(&mut self, ctx: &egui::Context) {
        // Don't open a second dialog if one is already pending
        if self.file_dialog_receiver.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        self.file_dialog_receiver = Some(receiver);

        let ctx_clone = ctx.clone();
        thread::spawn(move || {
            let result = rfd::AsyncFileDialog::new()
                .add_filter("Config files", &["json", "vps"])
                .add_filter("JSON", &["json"])
                .add_filter("Vitamins preset", &["vps"])
                .pick_file();

            // Block on the async result in this background thread
            let path = pollster::block_on(result).map(|handle| handle.path().to_path_buf());

            let _ = sender.send(path);
            // Wake up the UI thread so it checks the receiver
            ctx_clone.request_repaint();
        });
    }

    fn poll_file_dialog(&mut self) {
        if let Some(receiver) = &self.file_dialog_receiver {
            if let Ok(result) = receiver.try_recv() {
                if let Some(path) = result {
                    self.transform_path = path.to_string_lossy().to_string();
                }
                self.file_dialog_receiver = None;
            }
        }
    }

    fn connect(&mut self) {
        if !self.active.load(Ordering::Relaxed) {
            self.config_error = None;
            let config_path = Path::new(&self.transform_path);
            if !config_path.is_file() {
                self.config_error = Some(format!("Config file not found: {}", self.transform_path));
                return;
            }

            self.active.store(true, Ordering::Relaxed);
            self.packet_count.store(0, Ordering::Relaxed);
            self.last_packet_count = 0;
            self.packet_rate = 0.0;
            self.last_packet_time = Instant::now();

            let path = self.transform_path.clone();
            let ip = self.ip.clone();
            let face_search_timeout: i64 = self.face_search_timeout;

            let (tracking_sender, tracking_receiver): (
                Sender<TrackingResponse>,
                Receiver<TrackingResponse>,
            ) = mpsc::channel();
            let (plugin_sender, plugin_receiver): (
                Sender<TrackingResponse>,
                Receiver<TrackingResponse>,
            ) = mpsc::channel();

            let flag_pc = Arc::clone(&self.active);
            let flag_ph = Arc::clone(&self.active);
            let packet_counter = Arc::clone(&self.packet_count);
            let active_clone = Arc::clone(&self.active);

            let _ = thread::spawn(move || {
                while active_clone.load(Ordering::Relaxed) {
                    match tracking_receiver.recv_timeout(Duration::from_millis(200)) {
                        Ok(response) => {
                            packet_counter.fetch_add(1, Ordering::Relaxed);
                            if plugin_sender.send(response).is_err() {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });

            let _ = thread::spawn(move || {
                VTubeStudioPlugin::new(
                    plugin_receiver,
                    path,
                    0,
                    face_search_timeout.unsigned_abs(),
                )
                .run(flag_pc);
            });

            let function: fn(
                ip: String,
                sender: Sender<TrackingResponse>,
                active: Arc<AtomicBool>,
            );
            match self.tracking_client_type {
                TrackingClientType::VTubeStudio => function = VTubeStudioTrackingClient::run,
                TrackingClientType::IFacialMocap => function = IFacialMocapTrackingClinet::run,
            }
            let _ = thread::spawn(move || function(ip, tracking_sender, flag_ph));
        } else {
            self.active.store(false, Ordering::Relaxed);
            self.packet_count.store(0, Ordering::Relaxed);
            self.last_packet_count = 0;
            self.packet_rate = 0.0;
        }
    }
}

impl eframe::App for SnenkBridgeUI {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // Poll for file dialog results each frame
        self.poll_file_dialog();

        egui::CentralPanel::default().show(ctx, |ui| {
            let editing_enabled = !self.active.load(Ordering::Relaxed);
            ui.add_enabled_ui(editing_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Config File");
                    ui.text_edit_singleline(&mut self.transform_path);
                    let dialog_pending = self.file_dialog_receiver.is_some();
                    ui.add_enabled_ui(!dialog_pending, |ui| {
                        if ui.button("...").clicked() {
                            self.open_file_dialog(ctx);
                        }
                    });
                });

                ui.horizontal(|ui| {
                    ui.label("Phone IP");
                    ui.text_edit_singleline(&mut self.ip);
                });

                ui.horizontal(|ui| {
                    ui.label("Face search timeout (ms)");
                    ui.add(egui::DragValue::new(&mut self.face_search_timeout).range(0..=60_000));
                });

                ui.horizontal(|ui| {
                    ui.label("Tracking Type");
                    egui::ComboBox::from_label("Tracking Type")
                        .selected_text(format!("{:?}", self.tracking_client_type))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.tracking_client_type,
                                TrackingClientType::VTubeStudio,
                                "VTubeStudio",
                            );
                            ui.selectable_value(
                                &mut self.tracking_client_type,
                                TrackingClientType::IFacialMocap,
                                "IFacialMocap",
                            );
                        });
                });
            });

            if let Some(error) = &self.config_error {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, error);
                });
            }

            let now = Instant::now();
            let current_count = self.packet_count.load(Ordering::Relaxed);
            let elapsed = now.duration_since(self.last_packet_time);
            if elapsed.as_secs_f64() >= 0.5 {
                self.packet_rate = if current_count >= self.last_packet_count {
                    (current_count - self.last_packet_count) as f64 / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                self.last_packet_count = current_count;
                self.last_packet_time = now;
            }

            ui.horizontal(|ui| {
                ui.label(format!("Packets/s: {:.1}", self.packet_rate));
            });

            ui.horizontal(|ui| {
                let button_text = if self.active.load(Ordering::Relaxed) {
                    "Stop Tracking"
                } else {
                    "Start Tracking"
                };
                if ui.button(button_text).clicked() {
                    self.connect();
                }
            });
        });
    }
}
