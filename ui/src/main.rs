use eframe::egui;
use egui_file::FileDialog;
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
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self},
};
use tungstenite::connect;

fn main() {
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
    opened_file: Option<PathBuf>,
    #[serde(skip)]
    open_file_dialog: Option<FileDialog>,
}

impl Default for SnenkBridgeUI {
    fn default() -> Self {
        Self {
            transform_path: String::new(),
            ip: "127.0.0.1".to_string(),
            tracking_client_type: TrackingClientType::VTubeStudio,
            face_search_timeout: 3000,
            active: Arc::new(AtomicBool::new(false)),
            open_file_dialog: None,
            opened_file: None,
        }
    }
}

impl SnenkBridgeUI {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    fn connect(&self) {
        if !self.active.load(Ordering::Relaxed) {
            self.active.store(true, Ordering::Relaxed);
            let path = self.transform_path.clone();
            let ip = self.ip.clone();
            let face_search_timeout: i64 = self.face_search_timeout.clone();

            let (sender, receiver): (Sender<TrackingResponse>, Receiver<TrackingResponse>) =
                mpsc::channel();

            let flag_pc = Arc::clone(&self.active);
            let flag_ph = Arc::clone(&self.active);

            let _ = thread::spawn(move || {
                VTubeStudioPlugin::new(receiver, path, 0, face_search_timeout.unsigned_abs())
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
            let _ = thread::spawn(move || function(ip, sender, flag_ph));

            //Make everything uneditable while service is running
        } else {
            self.active.store(false, Ordering::Relaxed);

            //make everything editable again
        }
    }
}

impl eframe::App for SnenkBridgeUI {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Config File");
                ui.text_edit_singleline(&mut self.transform_path);
                if (ui.button("...")).clicked() {
                    let filter = Box::new({
                        let ext = Some(OsStr::new("json"));
                        move |path: &Path| -> bool { path.extension() == ext }
                    });

                    let mut dialog = FileDialog::open_file().show_files_filter(filter);
                    if let Some(mut path) = self.opened_file.clone() {
                        if path.pop() {
                            dialog = dialog.initial_path(path);
                        }
                    }

                    dialog.open();
                    self.open_file_dialog = Some(dialog);
                }

                if let Some(dialog) = &mut self.open_file_dialog {
                    if dialog.show(ctx).selected() {
                        if let Some(file) = dialog.path() {
                            self.opened_file = Some(file.to_path_buf());
                            self.transform_path = file.to_str().unwrap_or_default().into();
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Phone IP");
                ui.text_edit_singleline(&mut self.ip);
            });

            ui.horizontal(|ui| {
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
                    })
            });

            ui.horizontal(|ui| {
                if (ui.button("Start Tracking")).clicked() {
                    self.connect();
                }
            })
        });
    }
}

const TRACKING_CLIENT_TYPES: [TrackingClientType; 2] = [
    TrackingClientType::VTubeStudio,
    TrackingClientType::IFacialMocap,
];
