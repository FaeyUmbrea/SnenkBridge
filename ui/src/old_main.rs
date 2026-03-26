#![windows_subsystem = "windows"]
extern crate native_windows_derive as nwd;
extern crate native_windows_gui as nwg;

use std::{
    env, fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self},
};

use nwd::NwgUi;
use nwg::{NativeUi, NumberSelectData};
use sandoitchi_bridge_service::{
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vts::plugin::VTubeStudioPlugin,
};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UIConfig {
    transform_path: Option<String>,
    ip: Option<String>,
    tracking_client_type: Option<TrackingClientType>,
    face_search_timeout: Option<i64>,
}

const TRACKING_CLIENT_TYPES: [TrackingClientType; 2] = [
    TrackingClientType::VTubeStudio,
    TrackingClientType::IFacialMocap,
];

#[derive(Default, NwgUi)]
pub struct App {
    #[nwg_control(size: (300, 180), position: (300, 300), title: "Sandoitchi Bridge", flags: "WINDOW|VISIBLE", icon: data.embed.icon_str("APP_ICON", None).as_ref())]
    #[nwg_events( OnWindowClose: [App::close], OnInit: [App::init] )]
    window: nwg::Window,

    #[nwg_resource]
    embed: nwg::EmbedResource,

    #[nwg_control(size: (240, 25), position: (10, 12), placeholder_text: Some("Path to config file"))]
    transform_file_path: nwg::TextInput,

    #[nwg_control(text: "📃", size: (30, 30), position: (260, 10))]
    #[nwg_events( OnButtonClick: [App::open_file] )]
    file_button: nwg::Button,

    #[nwg_control(size: (280, 25), position: (10, 52), placeholder_text: Some("Phone IP (0.0.0.0)"))]
    phone_ip: nwg::TextInput,

    #[nwg_control(selected_index: Some(0), collection: Vec::from(TRACKING_CLIENT_TYPES), size: (170, 25), position: (10, 90))]
    tracking_client_type: nwg::ComboBox<TrackingClientType>,

    #[nwg_control(min_int: 0, max_int: 60_000, value_int: 3_000, size: (90, 25), position: (190, 90))]
    face_search_timeout: nwg::NumberSelect,

    #[nwg_control(text: "Connect", size: (280, 30), position: (10, 128))]
    #[nwg_events(OnButtonClick: [App::connect, App::save])]
    connect_button: nwg::Button,

    #[nwg_resource(size: 14)]
    label_font: nwg::Font,

    #[nwg_control(text: "https://github.com/an1by/SandoitchiBridge", position: (10, 158), size: (240, 15), font: Some(&data.label_font))]
    credits: nwg::Label,

    #[nwg_resource(action: FileDialogAction::Open, title: "Select Transfom File")]
    file_dialog: nwg::FileDialog,

    #[nwg_control(tip: Some("Sandoitchi Bridge"), icon : data.embed.icon_str("APP_ICON", None).as_ref())]
    #[nwg_events(MousePressLeftUp: [App::show_menu], OnContextMenu: [App::show_menu])]
    tray: nwg::TrayNotification,

    #[nwg_control(parent: window, popup: true)]
    tray_menu: nwg::Menu,

    #[nwg_control(parent: tray_menu, text: "Show")]
    #[nwg_events(OnMenuItemSelected: [App::show])]
    tray_show: nwg::MenuItem,

    #[nwg_control(parent: tray_menu, text: "Exit")]
    #[nwg_events(OnMenuItemSelected: [App::exit])]
    tray_exit: nwg::MenuItem,

    active: Arc<AtomicBool>,
}

impl App {
    fn init(&self) {
        if let Ok(last_config) = fs::read_to_string("ui-cfg.json") {
            let cfg = serde_json::from_str::<UIConfig>(&last_config).unwrap();

            if cfg.transform_path.is_some() {
                self.transform_file_path
                    .set_text(&cfg.transform_path.unwrap());
            }

            if cfg.ip.is_some() {
                self.phone_ip.set_text(&cfg.ip.unwrap());
            }

            if cfg.tracking_client_type.is_some() {
                if let Some(ref tracking_client_type) = cfg.tracking_client_type {
                    if let Some(index) = TRACKING_CLIENT_TYPES
                        .iter()
                        .position(|x| x == tracking_client_type)
                    {
                        self.tracking_client_type.set_selection(Some(index));
                    }
                }
            }

            if cfg.face_search_timeout.is_some() {
                let timeout = cfg.face_search_timeout.unwrap().abs();
                let data = NumberSelectData::Int {
                    value: timeout,
                    step: 1,
                    max: 60_000,
                    min: 0,
                };
                self.face_search_timeout.set_data(data);
            }
        };
    }

    fn save(&self) {
        let data: i64 = self
            .face_search_timeout
            .data()
            .formatted_value()
            .parse::<i64>()
            .unwrap();
        let config = UIConfig {
            transform_path: Some(self.transform_file_path.text()),
            ip: Some(self.phone_ip.text()),
            tracking_client_type: Some(
                TRACKING_CLIENT_TYPES
                    .get(self.tracking_client_type.selection().unwrap())
                    .unwrap()
                    .clone(),
            ),
            face_search_timeout: Some(data),
        };
        let config_str = serde_json::to_string(&config).unwrap();
        fs::write("ui-cfg.json", config_str).unwrap();
    }

    fn connect(&self) {
        if !self.active.load(Ordering::Relaxed) {
            self.active.store(true, Ordering::Relaxed);
            let path = self.transform_file_path.text().clone();
            let ip = self.phone_ip.text().clone();
            let face_search_timeout: i64 = self
                .face_search_timeout
                .data()
                .formatted_value()
                .parse::<i64>()
                .unwrap();

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
            let tracking_index = self.tracking_client_type.selection().unwrap();
            match TRACKING_CLIENT_TYPES.get(tracking_index).unwrap() {
                TrackingClientType::VTubeStudio => function = VTubeStudioTrackingClient::run,
                TrackingClientType::IFacialMocap => function = IFacialMocapTrackingClinet::run,
            }
            let _ = thread::spawn(move || function(ip, sender, flag_ph));

            self.transform_file_path.set_readonly(true);
            self.phone_ip.set_readonly(true);
            self.file_button.set_enabled(false);
            self.connect_button.set_text("Disconnect");
        } else {
            self.active.store(false, Ordering::Relaxed);

            self.transform_file_path.set_readonly(false);
            self.phone_ip.set_readonly(false);
            self.file_button.set_enabled(true);
            self.connect_button.set_text("Connect");
        }
    }

    fn open_file(&self) {
        if let Ok(d) = env::current_dir() {
            if let Some(d) = d.to_str() {
                self.file_dialog
                    .set_default_folder(d)
                    .expect("Failed to set default folder.");
            }
        }

        if self.file_dialog.run(Some(&self.window)) {
            {
                self.transform_file_path.set_text("");
                if let Ok(path) = self.file_dialog.get_selected_item() {
                    let dir = path.into_string().unwrap();
                    self.transform_file_path.set_text(&dir);
                }
            }
        };
    }

    fn close(&self) {
        self.window.minimize();
    }

    fn show_menu(&self) {
        let (x, y) = nwg::GlobalCursor::position();
        self.tray_menu.popup(x, y);
    }

    fn show(&self) {
        self.window.restore();
    }

    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }
}

fn main() {
    let log_config = include_str!("../../configs/log_cfg.yml");
    let raw_log_config = serde_yaml::from_str(log_config).unwrap();
    log4rs::init_raw_config(raw_log_config).unwrap();

    nwg::init().expect("Failed to init Native Windows GUI");
    nwg::Font::set_global_family("PT Sans").expect("Failed to set default font");

    let _app = App::build_ui(Default::default()).expect("Failed to build UI");

    nwg::dispatch_thread_events();
}
