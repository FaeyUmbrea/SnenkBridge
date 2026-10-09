#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(test)]
mod core_tests;
mod directories;
mod evaluation;
mod logging;
mod model;
mod network;
mod presets;
mod settings;
mod ui;

include!(concat!(env!("OUT_DIR"), "/credits.rs"));
include!(concat!(env!("OUT_DIR"), "/blendshapes.rs"));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init_logging();
    ui::run()
}
