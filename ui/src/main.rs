use iced::widget::{button, column, combo_box, container, row, text, text_input, Space};
use iced::{color, Border, Element, Length, Padding, Task, Theme};
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
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

// ─── Main ───────────────────────────────────────────────────────────

fn main() -> iced::Result {
    let log_config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../configs/log_cfg.yml");
    log4rs::init_file(log_config_path, Default::default())
        .expect("Unable to initialize logging from configs/log_cfg.yml");

    iced::application(App::new, App::update, App::view)
        .theme(app_theme)
        .window_size((520.0, 280.0))
        .run()
}

// ─── Theme ──────────────────────────────────────────────────────────

fn app_theme(_: &App) -> Theme {
    theme()
}

fn theme() -> Theme {
    let palette = iced::theme::Palette {
        background: color!(0x2b2b3d),
        text: color!(0xd4d4e8),
        primary: color!(0x6c6cb5),
        success: color!(0x5dba7d),
        warning: color!(0xffc14e),
        danger: color!(0xe05a5a),
    };
    Theme::custom("SnenkBridge".to_string(), palette)
}

// Widget style helpers

fn styled_button(label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0x4a4a7a))),
        text_color: color!(0xd4d4e8),
        border: Border {
            color: color!(0x6c6cb5),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn styled_button_hovered(label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0x5c5c90))),
        text_color: color!(0xeeeeff),
        border: Border {
            color: color!(0x8888cc),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn accent_button(_label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0x6c6cb5))),
        text_color: color!(0xffffff),
        border: Border {
            color: color!(0x8888cc),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn accent_button_hovered(_label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0x7e7ecc))),
        text_color: color!(0xffffff),
        border: Border {
            color: color!(0x9999dd),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn danger_button(_label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0x8b3a3a))),
        text_color: color!(0xffffff),
        border: Border {
            color: color!(0xcc5555),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn danger_button_hovered(_label: &str) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(color!(0xa04545))),
        text_color: color!(0xffffff),
        border: Border {
            color: color!(0xdd6666),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn input_style(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: iced::Background::Color(color!(0x1e1e2e)),
        border: Border {
            color: color!(0x4a4a6a),
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: color!(0x8888aa),
        placeholder: color!(0x6a6a8a),
        value: color!(0xd4d4e8),
        selection: color!(0x6c6cb5),
    }
}

fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(color!(0x232335))),
        border: Border {
            color: color!(0x3a3a55),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

// ─── State ──────────────────────────────────────────────────────────

struct App {
    transform_path: String,
    ip: String,
    tracking_types: combo_box::State<String>,
    selected_tracking_type: String,
    face_search_timeout: String,

    active: Arc<AtomicBool>,
    packet_count: Arc<AtomicUsize>,
    config_error: Option<String>,
    last_packet_count: usize,
    last_packet_time: Instant,
    packet_rate: f64,
}

// ─── Messages ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    TransformPathChanged(String),
    IpChanged(String),
    TrackingTypeSelected(String),
    TimeoutChanged(String),
    BrowseFile,
    FileSelected(Option<PathBuf>),
    ToggleConnection,
    Tick,
}

// ─── App implementation ─────────────────────────────────────────────

impl App {
    fn new() -> Self {
        let types = vec!["VTubeStudio".to_string(), "IFacialMocap".to_string()];
        Self {
            transform_path: String::new(),
            ip: "127.0.0.1".to_string(),
            tracking_types: combo_box::State::new(types),
            selected_tracking_type: "VTubeStudio".to_string(),
            face_search_timeout: "3000".to_string(),
            active: Arc::new(AtomicBool::new(false)),
            packet_count: Arc::new(AtomicUsize::new(0)),
            config_error: None,
            last_packet_count: 0,
            last_packet_time: Instant::now(),
            packet_rate: 0.0,
        }
    }

    fn tracking_client_type(&self) -> TrackingClientType {
        match self.selected_tracking_type.as_str() {
            "IFacialMocap" => TrackingClientType::IFacialMocap,
            _ => TrackingClientType::VTubeStudio,
        }
    }

    fn timeout_ms(&self) -> i64 {
        self.face_search_timeout.parse::<i64>().unwrap_or(3000)
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn connect(&mut self) {
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
        let face_search_timeout = self.timeout_ms();

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

        thread::spawn(move || {
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

        thread::spawn(move || {
            VTubeStudioPlugin::new(plugin_receiver, path, 0, face_search_timeout.unsigned_abs())
                .run(flag_pc);
        });

        let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>);
        match self.tracking_client_type() {
            TrackingClientType::VTubeStudio => function = VTubeStudioTrackingClient::run,
            TrackingClientType::IFacialMocap => function = IFacialMocapTrackingClinet::run,
        }
        thread::spawn(move || function(ip, tracking_sender, flag_ph));
    }

    fn disconnect(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        self.packet_count.store(0, Ordering::Relaxed);
        self.last_packet_count = 0;
        self.packet_rate = 0.0;
    }

    // ─── Update ─────────────────────────────────────────────────────

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TransformPathChanged(path) => {
                self.transform_path = path;
            }
            Message::IpChanged(ip) => {
                self.ip = ip;
            }
            Message::TrackingTypeSelected(t) => {
                self.selected_tracking_type = t;
            }
            Message::TimeoutChanged(val) => {
                // Only allow numeric input
                if val.is_empty() || val.chars().all(|c| c.is_ascii_digit()) {
                    self.face_search_timeout = val;
                }
            }
            Message::BrowseFile => {
                return Task::future(
                    rfd::AsyncFileDialog::new()
                        .add_filter("Config files", &["json", "vps"])
                        .add_filter("JSON", &["json"])
                        .add_filter("Vitamins preset", &["vps"])
                        .pick_file(),
                )
                .map(|handle| Message::FileSelected(handle.map(|h| h.path().to_path_buf())));
            }
            Message::FileSelected(path) => {
                if let Some(p) = path {
                    self.transform_path = p.to_string_lossy().to_string();
                }
            }
            Message::ToggleConnection => {
                if self.is_active() {
                    self.disconnect();
                } else {
                    self.connect();
                }
            }
            Message::Tick => {
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
            }
        }
        Task::none()
    }

    // ─── View ───────────────────────────────────────────────────────

    fn view(&self) -> Element<Message> {
        let editing = !self.is_active();

        // Config file row
        let file_input = text_input("Path to config file...", &self.transform_path)
            .on_input_maybe(editing.then_some(Message::TransformPathChanged))
            .style(input_style)
            .padding(10)
            .width(Length::Fill);

        let browse_btn = button(text("Browse").center())
            .on_press_maybe(editing.then_some(Message::BrowseFile))
            .style(|_theme, status| match status {
                button::Status::Hovered => styled_button_hovered(""),
                _ => styled_button(""),
            })
            .padding(Padding::from([8, 16]))
            .width(90);

        let file_row = row![file_input, browse_btn].spacing(8);

        // IP row
        let ip_label = text("Phone IP").size(14).color(color!(0x9999bb));
        let ip_input = text_input("192.168.1.71", &self.ip)
            .on_input_maybe(editing.then_some(Message::IpChanged))
            .style(input_style)
            .padding(10)
            .width(Length::Fill);

        let ip_row = row![ip_label, ip_input]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // Tracking type + timeout row
        let type_label = text("Tracking").size(14).color(color!(0x9999bb));
        let type_combo = combo_box(
            &self.tracking_types,
            "Select tracker...",
            Some(&self.selected_tracking_type),
            Message::TrackingTypeSelected,
        )
        .width(180)
        .padding(10);

        let timeout_label = text("Timeout").size(14).color(color!(0x9999bb));
        let timeout_input = text_input("3000", &self.face_search_timeout)
            .on_input_maybe(editing.then_some(Message::TimeoutChanged))
            .style(input_style)
            .padding(10)
            .width(80);
        let timeout_unit = text("ms").size(13).color(color!(0x6a6a8a));

        let tracking_row = row![
            type_label,
            type_combo,
            Space::new().width(Length::Fill),
            timeout_label,
            timeout_input,
            timeout_unit,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        // Error display
        let error_row: Element<Message> = if let Some(err) = &self.config_error {
            text(err).size(13).color(color!(0xe05a5a)).into()
        } else {
            Space::new().height(0).into()
        };

        // Status bar
        let status_text = if self.is_active() {
            text(format!("{:.1} packets/s", self.packet_rate))
                .size(13)
                .color(color!(0x5dba7d))
        } else {
            text("Disconnected").size(13).color(color!(0x6a6a8a))
        };

        // Connect/disconnect button
        let connect_btn = if self.is_active() {
            button(text("Disconnect").center())
                .on_press(Message::ToggleConnection)
                .style(|_theme, status| match status {
                    button::Status::Hovered => danger_button_hovered(""),
                    _ => danger_button(""),
                })
                .padding(Padding::from([10, 24]))
                .width(Length::Fill)
        } else {
            button(text("Connect").center())
                .on_press(Message::ToggleConnection)
                .style(|_theme, status| match status {
                    button::Status::Hovered => accent_button_hovered(""),
                    _ => accent_button(""),
                })
                .padding(Padding::from([10, 24]))
                .width(Length::Fill)
        };

        // Bottom bar
        let bottom_row = row![status_text, Space::new().width(Length::Fill), connect_btn]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // Main layout
        let content = column![
            file_row,
            ip_row,
            tracking_row,
            error_row,
            Space::new().height(Length::Fill),
            bottom_row,
        ]
        .spacing(12)
        .padding(20);

        container(content)
            .style(panel_style)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(0)
            .into()
    }
}
