use std::{
    path::PathBuf,
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
};

use clap::{Parser, Subcommand};
use snenk_bridge_service::{
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vitamins,
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
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // Legacy flat args for the `bridge` mode (backwards compat)
    #[arg(short, long, help = "Path to JSON config with transformations")]
    config: Option<String>,
    #[arg(short, long, help = "Phone IP address")]
    phone_ip: Option<String>,
    #[arg(
        short,
        long,
        value_parser = parse_tracking_client_type,
        help = "Tracking application type"
    )]
    tracking_client: Option<TrackingClientType>,
    #[arg(
        short,
        long,
        default_value_t = 3000,
        hide_default_value = true,
        help = "The time in milliseconds to wait before changing FaceFound to 0. Default: 3000"
    )]
    face_search_timeout: u64,
    #[arg(long, default_value = "localhost", help = "VTube Studio IP address")]
    vts_ip: String,
    #[arg(long, default_value = "8001", help = "VTube Studio API port")]
    vts_port: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Convert a Vitamins .vps preset file to SnenkBridge JSON format
    Convert {
        /// Path to the input .vps file
        input: PathBuf,
        /// Output path (defaults to input filename with .json extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Convert { input, output }) => {
            run_convert(input, output);
        }
        None => {
            run_bridge(cli);
        }
    }
}

fn run_convert(input: PathBuf, output: Option<PathBuf>) {
    let output = output.unwrap_or_else(|| input.with_extension("snek"));

    let content = match std::fs::read_to_string(&input) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading {}: {}", input.display(), e);
            std::process::exit(1);
        }
    };

    let preset = match vitamins::convert_vitamins_to_preset(&content, true) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error converting {}: {}", input.display(), e);
            std::process::exit(1);
        }
    };

    let json = match serde_json::to_string_pretty(&preset) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Error serializing: {}", e);
            std::process::exit(1);
        }
    };

    match std::fs::write(&output, &json) {
        Ok(_) => println!("Converted {} -> {}", input.display(), output.display()),
        Err(e) => {
            eprintln!("Error writing {}: {}", output.display(), e);
            std::process::exit(1);
        }
    }
}

fn run_bridge(cli: Cli) {
    let config_path = cli.config.unwrap_or_else(|| {
        eprintln!("Error: --config is required for bridge mode");
        std::process::exit(1);
    });
    let config = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("Error reading config {}: {}", config_path, e);
        std::process::exit(1);
    });
    let phone_ip = cli.phone_ip.unwrap_or_else(|| {
        eprintln!("Error: --phone-ip is required for bridge mode");
        std::process::exit(1);
    });
    let tracking_client = cli.tracking_client.unwrap_or_else(|| {
        eprintln!("Error: --tracking-client is required for bridge mode");
        std::process::exit(1);
    });

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
            config,
            cli.face_search_timeout,
            cli.vts_ip,
            cli.vts_port,
        )
        .run(active_flag);
    });

    let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>) = match tracking_client {
        TrackingClientType::VTubeStudio => VTubeStudioTrackingClient::run,
        TrackingClientType::IFacialMocap => IFacialMocapTrackingClinet::run,
    };
    let phonetr_handler = thread::spawn(move || function(phone_ip, sender, active_flag_clone));

    let _ = pctr_handler.join();
    let _ = phonetr_handler.join();
}
