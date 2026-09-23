#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(test)]
mod core_tests;
mod directories;
mod evaluation;
mod face_mesh;
mod logging;
mod model;
mod network;
mod presets;
mod renderer;
mod settings;
mod ui;

slint::include_modules!();
include!(concat!(env!("OUT_DIR"), "/credits.rs"));
include!(concat!(env!("OUT_DIR"), "/blendshapes.rs"));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init_logging();
    ui::run()
}
