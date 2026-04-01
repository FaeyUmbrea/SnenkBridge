use std::{
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
};

use clap::Parser;
use snenk_bridge_service::{
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vts::plugin::VTubeStudioPlugin,
};

fn parse_tracking_client_type(input: &str) -> Result<TrackingClientType, String> {
    match input.to_lowercase().as_str() {
        "vts" | "vtubestudio" => Ok(TrackingClientType::VTubeStudio),
        "ifm" | "ifacialmocap" => Ok(TrackingClientType::IFacialMocap),
        _ => Err(format!("Invalid tracking client type: {}", input)),
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, help = "Path to JSON config with transformations")]
    config: String,
    #[arg(short, long, help = "Phone IP address")]
    phone_ip: String,
    #[arg(
        short,
        long,
        value_parser = parse_tracking_client_type,
        help = "Tracking application type"
    )]
    tracking_client: TrackingClientType,
    #[arg(
        short,
        long,
        default_value_t = 3000,
        hide_default_value = true,
        help = "The time in milliseconds to wait before changing FaceFound to 0. Default: 3000"
    )]
    face_search_timeout: u64,
    #[arg(
        short = 'd',
        long,
        default_value_t = 0,
        hide_default_value = true,
        help = "Optional delay for config reloading in milliseconds. Default: 0 (disabled)"
    )]
    config_reload_delay: u64,
    #[arg(long, default_value = "localhost", help = "VTube Studio IP address")]
    vts_ip: String,
    #[arg(long, default_value = "8001", help = "VTube Studio API port")]
    vts_port: String,
}

fn main() {
    let args = Args::parse();

    println!("Github: https://github.com/FaeyUmbrea/SnenkBridge");

    let active_flag = Arc::new(AtomicBool::new(true));
    let active_flag_clone = Arc::clone(&active_flag);

    let log_config = include_str!("../configs/log_cfg.yml");
    let raw_log_config = serde_yaml::from_str(log_config).unwrap();
    log4rs::init_raw_config(raw_log_config).unwrap();

    let (sender, receiver): (Sender<TrackingResponse>, Receiver<TrackingResponse>) =
        mpsc::channel();

    let pctr_handler = thread::spawn(move || {
        VTubeStudioPlugin::new(
            receiver,
            args.config,
            args.config_reload_delay,
            args.face_search_timeout,
            args.vts_ip,
            args.vts_port,
        )
        .run(active_flag);
    });

    let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>) = match args.tracking_client
    {
        TrackingClientType::VTubeStudio => VTubeStudioTrackingClient::run,
        TrackingClientType::IFacialMocap => IFacialMocapTrackingClinet::run,
    };
    let phonetr_handler = thread::spawn(move || function(args.phone_ip, sender, active_flag_clone));

    let _ = pctr_handler.join();
    let _ = phonetr_handler.join();
}
