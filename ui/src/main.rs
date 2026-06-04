// Hide the Windows console window in release builds. Debug builds keep it so
// `cargo run` still shows live stdout logging. Logs are also written to
// `log/log.log`, so no logging is lost when the console is hidden.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use i_slint_backend_winit::WinitWindowAccessor;
use slint::Model;
use snenk_bridge_service::{
    preset::{self, SnekPreset},
    tracking::{
        client::{TrackingClient, TrackingClientType},
        ifacialmocap::IFacialMocapTrackingClinet,
        response::TrackingResponse,
        vtubestudio::VTubeStudioTrackingClient,
    },
    vitamins,
    vts::plugin::VTubeStudioPlugin,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

slint::include_modules!();

mod editor;
mod face_mesh;
mod logging;
mod presets;
mod preview;
mod settings;
mod util;

use editor::*;
use logging::init_logging;
use presets::*;
use preview::*;
use settings::*;
use util::*;

include!(concat!(env!("OUT_DIR"), "/credits.rs"));
include!(concat!(env!("OUT_DIR"), "/blendshapes.rs"));

fn main() {
    init_logging();

    let rt = tokio::runtime::Runtime::new().unwrap();

    let app = App::new().unwrap();

    let settings = load_settings();

    // Build preset list and find index of saved preset name
    let entries = refresh_preset_list(&app);
    let saved_index = entries
        .iter()
        .position(|e| e.name == settings.preset_name)
        .unwrap_or(0);

    let preset_list: Arc<Mutex<Vec<PresetEntry>>> = Arc::new(Mutex::new(entries));

    app.set_preset_index(i32::try_from(saved_index).unwrap_or(0));
    app.set_can_delete_preset(!is_builtin(&settings.preset_name));
    app.set_phone_ip(settings.phone_ip.into());
    app.set_tracking_type_index(settings.tracking_type_index);
    app.set_face_search_timeout(settings.face_search_timeout.into());
    app.set_vts_ip(settings.vts_ip.into());
    app.set_vts_port(settings.vts_port.into());

    // --- Input shapes model (Task 8) ---
    let input_model: Rc<slint::VecModel<NameValuePair>> = Rc::new(slint::VecModel::from(
        BLENDSHAPE_NAMES
            .iter()
            .map(|name| NameValuePair {
                name: (*name).into(),
                value: 0.0,
                min: 0.0,
                max: 1.0,
            })
            .collect::<Vec<_>>(),
    ));
    app.set_input_shapes(input_model.clone().into());

    // --- Editor params model (Task 8) ---
    let editor_params_model: Rc<slint::VecModel<EditorParam>> =
        Rc::new(slint::VecModel::from(Vec::<EditorParam>::new()));
    app.set_editor_params(editor_params_model.clone().into());

    // Persistent face point-cloud + output-panel models, updated in place by
    // the render loop.
    app.set_mesh_points(mesh_model().into());
    app.set_output_params(output_model().into());

    // Name of the preset currently loaded in the editor. Tracked explicitly
    // rather than inferred from the editable title field, so the switch guard
    // doesn't misfire when the title is edited or an unrelated setting changes.
    let loaded_preset: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // Load initial preset into editor and compute initial outputs.
    load_preset_into_editor(
        &app,
        &settings.preset_name,
        &editor_params_model,
        &loaded_preset,
    );
    refresh_editor_preview(&app, &editor_params_model, &input_model);

    // Shared tracking values for live preview (Task 10)
    let live_values: Arc<Mutex<HashMap<String, f64>>> = Arc::new(Mutex::new(HashMap::new()));

    let source_active = Arc::new(AtomicBool::new(false));
    let target_active = Arc::new(AtomicBool::new(false));
    let packet_count = Arc::new(AtomicUsize::new(0));

    // Shared sender for the bridge. Target creates plugin channels and stores
    // the sender here; the source bridge forwards tracking data through it.
    let plugin_tx: Arc<Mutex<Option<Sender<TrackingResponse>>>> = Arc::new(Mutex::new(None));

    // Settings changed -> persist + reload editor if preset changed (Task 9 guard)
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        let editor_params_model = Rc::clone(&editor_params_model);
        let loaded_preset = Rc::clone(&loaded_preset);
        let input_model = Rc::clone(&input_model);
        app.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = slint_idx(ui.get_preset_index());
            let is_custom = list.get(idx).is_some_and(|e| !is_builtin(&e.name));
            ui.set_can_delete_preset(is_custom);
            let preset_name = list
                .get(idx)
                .map_or_else(default_preset_name, |e| e.name.clone());
            drop(list);
            save_settings(&read_settings_from_ui(&ui, &preset_list.lock().unwrap()));

            // Only act when the selected preset differs from the one actually
            // loaded in the editor. Comparing against the loaded-preset name
            // (not the editable title) avoids misfiring on title edits or
            // unrelated setting changes.
            if preset_name != *loaded_preset.borrow() {
                if ui.get_has_unsaved_changes() {
                    // Show unsaved changes guard dialog (Task 9)
                    let dialog = UnsavedChangesDialog::new().unwrap();
                    let dialog_weak = dialog.as_weak();
                    let ui_weak = ui.as_weak();
                    let preset_name_clone = preset_name.clone();
                    let editor_params_model_clone = Rc::clone(&editor_params_model);
                    let loaded_preset_clone = Rc::clone(&loaded_preset);
                    let input_model_clone = Rc::clone(&input_model);
                    dialog.on_do_save(move || {
                        let Some(dlg) = dialog_weak.upgrade() else { return };
                        let Some(ui) = ui_weak.upgrade() else { return };
                        // Save current editor state before switching
                        let snek = build_snek_from_editor(&ui);
                        let dir = presets_dir();
                        if let Err(e) = preset::save_preset(&dir, &snek) {
                            ui.set_error_text(format!("Failed to save preset: {e}").into());
                        }
                        ui.set_has_unsaved_changes(false);
                        load_preset_into_editor(
                            &ui,
                            &preset_name_clone,
                            &editor_params_model_clone,
                            &loaded_preset_clone,
                        );
                        refresh_editor_preview(&ui, &editor_params_model_clone, &input_model_clone);
                        let _ = dlg.hide();
                    });
                    let dialog_weak2 = dialog.as_weak();
                    let ui_weak2 = ui.as_weak();
                    let preset_name_clone2 = preset_name.clone();
                    let editor_params_model_clone2 = Rc::clone(&editor_params_model);
                    let loaded_preset_clone2 = Rc::clone(&loaded_preset);
                    let input_model_clone2 = Rc::clone(&input_model);
                    dialog.on_do_discard(move || {
                        let Some(dlg) = dialog_weak2.upgrade() else { return };
                        let Some(ui) = ui_weak2.upgrade() else { return };
                        ui.set_has_unsaved_changes(false);
                        load_preset_into_editor(
                            &ui,
                            &preset_name_clone2,
                            &editor_params_model_clone2,
                            &loaded_preset_clone2,
                        );
                        refresh_editor_preview(&ui, &editor_params_model_clone2, &input_model_clone2);
                        let _ = dlg.hide();
                    });
                    let dialog_weak3 = dialog.as_weak();
                    dialog.on_do_cancel(move || {
                        if let Some(dlg) = dialog_weak3.upgrade() {
                            let _ = dlg.hide();
                        }
                    });
                    dialog.show().unwrap();
                } else {
                    load_preset_into_editor(
                        &ui,
                        &preset_name,
                        &editor_params_model,
                        &loaded_preset,
                    );
                    refresh_editor_preview(&ui, &editor_params_model, &input_model);
                }
            }
        });
    }

    // About window
    {
        app.on_show_about(move || {
            let about = AboutWindow::new().unwrap();
            about.set_credits_text(DEPENDENCY_CREDITS.into());
            about.show().unwrap();
        });
    }

    // Expression variables reference
    {
        let editor_params_model = Rc::clone(&editor_params_model);
        app.on_editor_show_variables(move || {
            let Ok(win) = VariablesWindow::new() else { return };

            let header = |t: &str| VarEntry {
                name: t.into(),
                note: String::new().into(),
                header: true,
            };
            let item = |n: &str, note: &str| VarEntry {
                name: n.into(),
                note: note.into(),
                header: false,
            };

            let mut entries: Vec<VarEntry> = Vec::new();

            entries.push(header("Face blendshapes (0–1)"));
            entries.extend(BLENDSHAPE_NAMES.iter().map(|n| item(n, "")));

            entries.push(header("Head"));
            entries.push(item("HeadPosX", "head position X"));
            entries.push(item("HeadPosY", "head position Y"));
            entries.push(item("HeadPosZ", "head position Z"));
            entries.push(item("HeadRotX", "head rotation (pitch)"));
            entries.push(item("HeadRotY", "head rotation (yaw)"));
            entries.push(item("HeadRotZ", "head rotation (roll)"));

            entries.push(header("Special"));
            entries.push(item("FaceFound", "1 while a face is tracked, else 0"));
            entries.push(item("Wave⟨ms⟩", "0→1→0 triangle over ⟨ms⟩, e.g. Wave2000"));
            entries.push(item("PingPong⟨ms⟩", "0→1 sawtooth over ⟨ms⟩, e.g. PingPong1000"));

            // The current preset's own params, usable as delay-buffer references.
            let names: Vec<String> = (0..editor_params_model.row_count())
                .filter_map(|i| editor_params_model.row_data(i))
                .map(|p| p.name.to_string())
                .filter(|n| !n.is_empty())
                .collect();
            if !names.is_empty() {
                entries.push(header("This preset's parameters (delay-buffer reference)"));
                entries.extend(names.iter().map(|n| item(n, "")));
            }

            win.set_entries(Rc::new(slint::VecModel::from(entries)).into());
            let _ = win.show();
        });
    }

    // Import preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_import_preset(move || {
            let weak = weak.clone();
            let preset_list = Arc::clone(&preset_list);
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("All presets", &["snek", "json", "vps"])
                    .add_filter("SnenkBridge preset", &["snek"])
                    .add_filter("Vitamins preset", &["vps"])
                    .add_filter("JSON", &["json"])
                    .pick_file();
                let Some(path) = file else { return };

                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(format!("Failed to read file: {e}").into());
                            }
                        });
                        return;
                    }
                };

                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let is_vps = ext == "vps";

                // Pre-parse to extract metadata
                let (title, author, description) = if is_vps {
                    // Try a quick parse for VPS metadata
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(val) => {
                            let t = val
                                .get("saveName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let a = val
                                .get("author")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            (t, a, String::new())
                        }
                        Err(_) => (String::new(), String::new(), String::new()),
                    }
                } else {
                    // .snek or .json
                    match preset::load_from_str(&content) {
                        Ok(p) => (p.title, p.author, p.description),
                        Err(_) => (String::new(), String::new(), String::new()),
                    }
                };

                // Default title from filename if empty
                let title = if title.is_empty() {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Imported")
                        .to_string()
                } else {
                    title
                };

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };

                    let dialog = ImportDialog::new().unwrap();
                    dialog.set_preset_title(title.into());
                    dialog.set_preset_author(author.into());
                    dialog.set_preset_description(description.into());
                    dialog.set_show_swap_toggle(is_vps);
                    dialog.set_swap_xy(false);

                    // do-import
                    {
                        let dialog_weak = dialog.as_weak();
                        let ui_weak = ui.as_weak();
                        let content = content.clone();
                        let preset_list = Arc::clone(&preset_list);
                        dialog.on_do_import(move || {
                            let Some(dlg) = dialog_weak.upgrade() else {
                                return;
                            };
                            let Some(ui) = ui_weak.upgrade() else {
                                return;
                            };

                            let title = dlg.get_preset_title().to_string();
                            let author = dlg.get_preset_author().to_string();
                            let description = dlg.get_preset_description().to_string();
                            let swap_xy = dlg.get_swap_xy();

                            if title.is_empty() {
                                ui.set_error_text("Preset title cannot be empty.".into());
                                return;
                            }

                            // Build the SnekPreset
                            let result: Result<SnekPreset, String> = if is_vps {
                                vitamins::convert_vitamins_to_preset(&content, swap_xy)
                            } else {
                                preset::load_from_str(&content)
                            };

                            match result {
                                Ok(mut snek) => {
                                    snek.title.clone_from(&title);
                                    snek.author = author;
                                    snek.description = description;

                                    let dir = presets_dir();
                                    match preset::save_preset(&dir, &snek) {
                                        Ok(_) => {
                                            let entries = refresh_preset_list(&ui);
                                            let new_idx = entries
                                                .iter()
                                                .position(|e| e.name == title)
                                                .unwrap_or(0);
                                            *preset_list.lock().unwrap() = entries;
                                            ui.set_preset_index(
                                                i32::try_from(new_idx).unwrap_or(0),
                                            );
                                            ui.set_can_delete_preset(true);
                                            ui.set_error_text("".into());
                                            save_settings(&read_settings_from_ui(
                                                &ui,
                                                &preset_list.lock().unwrap(),
                                            ));
                                        }
                                        Err(e) => {
                                            ui.set_error_text(
                                                format!("Failed to save preset: {e}").into(),
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    ui.set_error_text(
                                        format!("Failed to parse preset: {e}").into(),
                                    );
                                }
                            }

                            let _ = dlg.hide();
                        });
                    }

                    // do-cancel
                    {
                        let dialog_weak = dialog.as_weak();
                        dialog.on_do_cancel(move || {
                            if let Some(dlg) = dialog_weak.upgrade() {
                                let _ = dlg.hide();
                            }
                        });
                    }

                    dialog.show().unwrap();
                });
            });
        });
    }

    // Export preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        app.on_export_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = slint_idx(ui.get_preset_index());
            let name = list
                .get(idx)
                .map_or_else(default_preset_name, |e| e.name.clone());
            drop(list);

            let weak = ui.as_weak();
            std::thread::spawn(move || {
                let snek = match build_snek_preset(&name) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(format!("Export failed: {e}").into());
                            }
                        });
                        return;
                    }
                };

                let default_name = format!("{}.snek", preset::sanitize_title(&snek.title));
                let file = rfd::FileDialog::new()
                    .add_filter("SnenkBridge preset", &["snek"])
                    .set_file_name(&default_name)
                    .save_file();

                if let Some(path) = file {
                    let json = match serde_json::to_string_pretty(&snek) {
                        Ok(j) => j,
                        Err(e) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_error_text(
                                        format!("Failed to serialize preset: {e}").into(),
                                    );
                                }
                            });
                            return;
                        }
                    };
                    if let Err(e) = std::fs::write(&path, json) {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_error_text(format!("Failed to write file: {e}").into());
                            }
                        });
                    }
                }
            });
        });
    }

    // Delete preset
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        let editor_params_model = Rc::clone(&editor_params_model);
        let loaded_preset = Rc::clone(&loaded_preset);
        let input_model = Rc::clone(&input_model);
        app.on_delete_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let list = preset_list.lock().unwrap();
            let idx = slint_idx(ui.get_preset_index());
            let Some(entry) = list.get(idx) else { return };

            if is_builtin(&entry.name) {
                return;
            }

            let Some(filename) = entry.filename.clone() else {
                return;
            };
            drop(list);

            let dir = presets_dir();
            if let Err(e) = preset::delete_preset(&dir, &filename) {
                ui.set_error_text(format!("Failed to delete preset: {e}").into());
                return;
            }

            let entries = refresh_preset_list(&ui);
            *preset_list.lock().unwrap() = entries;
            ui.set_preset_index(0); // Select Default
            ui.set_can_delete_preset(false);
            ui.set_error_text("".into());
            save_settings(&read_settings_from_ui(&ui, &preset_list.lock().unwrap()));

            // Reflect the now-selected Default in the editor.
            load_preset_into_editor(
                &ui,
                &default_preset_name(),
                &editor_params_model,
                &loaded_preset,
            );
            refresh_editor_preview(&ui, &editor_params_model, &input_model);
        });
    }

    // New preset dialog (Task 8)
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        let editor_params_model = Rc::clone(&editor_params_model);
        let loaded_preset = Rc::clone(&loaded_preset);
        let input_model = Rc::clone(&input_model);
        app.on_new_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let dialog = NewPresetDialog::new().unwrap();

            // Populate base preset names: "Empty" + all existing presets
            let list = preset_list.lock().unwrap();
            let mut base_names: Vec<slint::SharedString> =
                vec![slint::SharedString::from("Empty")];
            for entry in list.iter() {
                base_names.push(entry.name.clone().into());
            }
            drop(list);
            dialog
                .set_base_preset_names(Rc::new(slint::VecModel::from(base_names)).into());
            dialog.set_base_preset_index(0);
            dialog.set_preset_title("New Preset".into());
            dialog.set_preset_author("".into());
            dialog.set_preset_description("".into());

            // on_do_create
            {
                let dialog_weak = dialog.as_weak();
                let ui_weak = ui.as_weak();
                let preset_list = Arc::clone(&preset_list);
                let editor_params_model = Rc::clone(&editor_params_model);
                let loaded_preset = Rc::clone(&loaded_preset);
                let input_model = Rc::clone(&input_model);
                dialog.on_do_create(move || {
                    let Some(dlg) = dialog_weak.upgrade() else { return };
                    let Some(ui) = ui_weak.upgrade() else { return };

                    let title = dlg.get_preset_title().to_string();
                    let author = dlg.get_preset_author().to_string();
                    let description = dlg.get_preset_description().to_string();
                    let base_idx = dlg.get_base_preset_index();

                    if title.is_empty() {
                        ui.set_error_text("Preset title cannot be empty.".into());
                        return;
                    }

                    // base_idx 0 = "Empty", 1+ = preset at (base_idx - 1)
                    let params: Vec<vitamins::CalcFn> = if base_idx == 0 {
                        Vec::new()
                    } else {
                        let list = preset_list.lock().unwrap();
                        let preset_idx = usize::try_from(base_idx - 1).unwrap_or(0);
                        let base_name = list
                            .get(preset_idx)
                            .map(|e| e.name.clone())
                            .unwrap_or_else(default_preset_name);
                        drop(list);
                        match build_snek_preset(&base_name) {
                            Ok(s) => s.params,
                            Err(_) => Vec::new(),
                        }
                    };

                    let mut snek = SnekPreset::new(title.clone(), params);
                    snek.author = author;
                    snek.description = description;

                    let dir = presets_dir();
                    match preset::save_preset(&dir, &snek) {
                        Ok(_) => {
                            let entries = refresh_preset_list(&ui);
                            let new_idx =
                                entries.iter().position(|e| e.name == title).unwrap_or(0);
                            *preset_list.lock().unwrap() = entries;
                            ui.set_preset_index(i32::try_from(new_idx).unwrap_or(0));
                            ui.set_can_delete_preset(true);
                            ui.set_error_text("".into());
                            save_settings(&read_settings_from_ui(
                                &ui,
                                &preset_list.lock().unwrap(),
                            ));
                            load_preset_into_editor(
                                &ui,
                                &title,
                                &editor_params_model,
                                &loaded_preset,
                            );
                            refresh_editor_preview(&ui, &editor_params_model, &input_model);
                        }
                        Err(e) => {
                            ui.set_error_text(format!("Failed to save preset: {e}").into());
                        }
                    }

                    let _ = dlg.hide();
                });
            }

            // on_do_cancel
            {
                let dialog_weak = dialog.as_weak();
                dialog.on_do_cancel(move || {
                    if let Some(dlg) = dialog_weak.upgrade() {
                        let _ = dlg.hide();
                    }
                });
            }

            dialog.show().unwrap();
        });
    }

    // Editor callbacks (Task 8 / 9)

    // editor_param_changed
    {
        let weak = app.as_weak();
        let editor_params_model = Rc::clone(&editor_params_model);
        let input_model = Rc::clone(&input_model);
        app.on_editor_param_changed(move |index, param| {
            let Some(ui) = weak.upgrade() else { return };
            let idx = slint_idx(index);
            editor_params_model.set_row_data(idx, param);
            ui.set_has_unsaved_changes(true);
            refresh_editor_preview(&ui, &editor_params_model, &input_model);
        });
    }

    // editor_add_param
    {
        let weak = app.as_weak();
        let editor_params_model = Rc::clone(&editor_params_model);
        let input_model = Rc::clone(&input_model);
        app.on_editor_add_param(move || {
            let Some(ui) = weak.upgrade() else { return };
            editor_params_model.push(calc_fn_to_editor_param(&vitamins::CalcFn {
                name: "NewParam".to_string(),
                func: String::new(),
                min: 0.0,
                max: 1.0,
                default_value: 0.0,
                delay_buffer: None,
            }));
            ui.set_has_unsaved_changes(true);
            refresh_editor_preview(&ui, &editor_params_model, &input_model);
        });
    }

    // editor_add_delay_param
    {
        let weak = app.as_weak();
        let editor_params_model = Rc::clone(&editor_params_model);
        let input_model = Rc::clone(&input_model);
        app.on_editor_add_delay_param(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Defaults mirror the typical delay buffers shipped in presets.
            editor_params_model.push(calc_fn_to_editor_param(&vitamins::CalcFn {
                name: "NewDelayParam".to_string(),
                func: String::new(),
                min: -10.0,
                max: 10.0,
                default_value: 0.0,
                delay_buffer: Some(vitamins::DelayBuffer {
                    ref_param: String::new(),
                    smoothing: 2.0,
                    delay_count: 8,
                    in_min: -30.0,
                    in_max: 30.0,
                    out_min: -10.0,
                    out_max: 10.0,
                }),
            }));
            ui.set_has_unsaved_changes(true);
            refresh_editor_preview(&ui, &editor_params_model, &input_model);
        });
    }

    // editor_delete_param
    {
        let weak = app.as_weak();
        let editor_params_model = Rc::clone(&editor_params_model);
        let input_model = Rc::clone(&input_model);
        app.on_editor_delete_param(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let idx = slint_idx(index);
            if idx < editor_params_model.row_count() {
                editor_params_model.remove(idx);
                ui.set_has_unsaved_changes(true);
                refresh_editor_preview(&ui, &editor_params_model, &input_model);
            }
        });
    }

    // editor_metadata_changed
    {
        let weak = app.as_weak();
        app.on_editor_metadata_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_has_unsaved_changes(true);
        });
    }

    // editor_save
    {
        let weak = app.as_weak();
        let preset_list = Arc::clone(&preset_list);
        let editor_params_model = Rc::clone(&editor_params_model);
        let loaded_preset = Rc::clone(&loaded_preset);
        let input_model = Rc::clone(&input_model);
        app.on_editor_save(move || {
            let Some(ui) = weak.upgrade() else { return };
            let snek = build_snek_from_editor_model(&ui, &editor_params_model);
            let title = snek.title.clone();
            let dir = presets_dir();
            match preset::save_preset(&dir, &snek) {
                Ok(_) => {
                    let entries = refresh_preset_list(&ui);
                    let new_idx = entries.iter().position(|e| e.name == title).unwrap_or(0);
                    *preset_list.lock().unwrap() = entries;
                    ui.set_preset_index(i32::try_from(new_idx).unwrap_or(0));
                    // The editor now reflects the saved preset (title may have
                    // changed); keep the loaded-preset marker in sync so the
                    // switch guard doesn't misfire on the next setting change.
                    *loaded_preset.borrow_mut() = title.clone();
                    ui.set_can_delete_preset(true);
                    ui.set_has_unsaved_changes(false);
                    ui.set_error_text("".into());
                    save_settings(&read_settings_from_ui(&ui, &preset_list.lock().unwrap()));
                    refresh_editor_preview(&ui, &editor_params_model, &input_model);
                }
                Err(e) => {
                    ui.set_error_text(format!("Failed to save preset: {e}").into());
                }
            }
        });
    }

    // Toggle source
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let packet_count = Arc::clone(&packet_count);
        let plugin_tx = Arc::clone(&plugin_tx);
        let live_values = Arc::clone(&live_values);
        let rt_handle = rt.handle().clone();

        app.on_toggle_source(move || {
            let Some(ui) = weak.upgrade() else { return };

            if source_active.load(Ordering::Relaxed) {
                source_active.store(false, Ordering::Relaxed);
                packet_count.store(0, Ordering::Relaxed);
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

            let (tracking_tx, tracking_rx) = mpsc::channel::<TrackingResponse>();
            let flag_tracking = Arc::clone(&source_active);
            let pkt_counter = Arc::clone(&packet_count);
            let ptx = Arc::clone(&plugin_tx);
            let source_flag_bridge = Arc::clone(&source_active);
            let live_values_bridge = Arc::clone(&live_values);

            // Tracking thread
            rt_handle.spawn_blocking(move || {
                let function: fn(String, Sender<TrackingResponse>, Arc<AtomicBool>) =
                    match tracking_type {
                        TrackingClientType::VTubeStudio => VTubeStudioTrackingClient::run,
                        TrackingClientType::IFacialMocap => IFacialMocapTrackingClinet::run,
                    };
                function(phone_ip, tracking_tx, flag_tracking);
            });

            // Bridge: reads from tracking_rx, forwards to plugin_tx, counts packets,
            // and stores latest blendshape values for live preview (Task 10).
            rt_handle.spawn(async move {
                loop {
                    if !source_flag_bridge.load(Ordering::Relaxed) {
                        break;
                    }
                    match tracking_rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(response) => {
                            pkt_counter.fetch_add(1, Ordering::Relaxed);

                            // Store latest values for live preview
                            {
                                let mut map = live_values_bridge.lock().unwrap();
                                for shape in &response.blend_shapes {
                                    map.insert(shape.k.clone(), shape.v);
                                }
                                map.insert("HeadRotX".to_string(), response.rotation.x);
                                map.insert("HeadRotY".to_string(), response.rotation.y);
                                map.insert("HeadRotZ".to_string(), response.rotation.z);
                                map.insert("HeadPosX".to_string(), response.position.x);
                                map.insert("HeadPosY".to_string(), response.position.y);
                                map.insert("HeadPosZ".to_string(), response.position.z);
                                map.insert(
                                    "FaceFound".to_string(),
                                    if response.face_found { 1.0 } else { 0.0 },
                                );
                            }

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

    // Toggle target
    {
        let weak = app.as_weak();
        let target_active = Arc::clone(&target_active);
        let plugin_tx = Arc::clone(&plugin_tx);
        let rt_handle = rt.handle().clone();
        let preset_list = Arc::clone(&preset_list);

        app.on_toggle_target(move || {
            let Some(ui) = weak.upgrade() else { return };

            if target_active.load(Ordering::Relaxed) {
                target_active.store(false, Ordering::Relaxed);
                *plugin_tx.lock().unwrap() = None;
                ui.set_target_active(false);
                ui.set_target_status("Disconnected".into());
                ui.set_target_status_color(color(COLOR_RED));
                return;
            }

            let list = preset_list.lock().unwrap();
            let idx = slint_idx(ui.get_preset_index());
            let preset_name = list
                .get(idx)
                .map_or_else(default_preset_name, |e| e.name.clone());
            drop(list);

            let config_json = match resolve_preset(&preset_name) {
                Ok(json) => json,
                Err(e) => {
                    ui.set_error_text(e.into());
                    return;
                }
            };

            ui.set_error_text("".into());
            ui.set_target_status("Connecting...".into());
            ui.set_target_status_color(color(COLOR_YELLOW));
            target_active.store(true, Ordering::Relaxed);
            ui.set_target_active(true);

            let face_search_timeout = timeout_ms(ui.get_face_search_timeout().as_ref());
            let vts_ip = ui.get_vts_ip().to_string();
            let vts_port = ui.get_vts_port().to_string();

            // Create fresh plugin channels; bridge will pick up the new sender
            let (tx, receiver) = mpsc::channel::<TrackingResponse>();
            *plugin_tx.lock().unwrap() = Some(tx);

            let flag = Arc::clone(&target_active);

            rt_handle.spawn_blocking(move || {
                VTubeStudioPlugin::new(
                    receiver,
                    config_json,
                    face_search_timeout,
                    vts_ip,
                    vts_port,
                )
                .run(flag);
            });
        });
    }

    // Status polling (Task 10: update input model from live values)
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let target_active = Arc::clone(&target_active);
        let packet_count = Arc::clone(&packet_count);
        let input_model = Rc::clone(&input_model);

        let src_had_data = Arc::new(AtomicBool::new(false));
        let tgt_ticks = Arc::new(AtomicUsize::new(0));

        // We need to move the Rc models into the async task via the event loop.
        // Wrap them in a way the poll closure can access them.
        // Since invoke_from_event_loop runs on the UI thread, we can use Rc there.
        // We capture them as weak references through a shared thread-local approach:
        // use Arc<Mutex<Option<...>>> isn't possible for Rc. Instead, we'll clone
        // the Rc only when invoking and use a static-lifetime trick via the weak handle.

        // Actually, Rc is not Send. We must do all model access inside invoke_from_event_loop.
        // The pattern is: capture input_model and editor_params_model as Rc in the closure
        // that runs on the event loop. Since invoke_from_event_loop takes a 'static closure,
        // we need to use slint's weak handle pattern.
        //
        // The cleanest approach: store the Rc models in a thread_local or pass them
        // via an Arc<Mutex<Weak>> wrapper. Since Rc is not Send, we'll use a different
        // approach: store raw pointers — but that's unsafe.
        //
        // Best approach: use slint::Weak for the models or simply keep them alive via
        // the App weak handle and re-fetch from the UI. But get_editor_params() returns
        // ModelRc which is a reference-counted pointer (it IS Send if the inner type
        // is thread-safe). Let's check: ModelRc is an Rc-based type in Slint.
        //
        // The correct pattern for this case is to capture the Rc values inside the
        // invoke_from_event_loop closure by sending them as non-Send data through the
        // event loop. We use a trick: wrap in a struct that is Send.
        //
        // Simpler: we don't actually need to capture these Rc values from outside the
        // event loop — we can re-derive them inside the closure from the App weak handle
        // because App::get_editor_params() and get_input_shapes() return ModelRc
        // (which contains the same underlying data). But we need the specific VecModel
        // to call set_row_data.
        //
        // The actual solution: use invoke_from_event_loop with a closure that captures
        // the Rc values directly. This is possible because invoke_from_event_loop
        // requires 'static, but on the main thread Rc IS available. We need to use
        // the unsafe send wrapper or restructure.
        //
        // PRACTICAL SOLUTION: Use a thread-safe shared state (Arc<Mutex<Vec<(String,f64)>>>)
        // for the live values (already done above), and in the event loop closure,
        // use the weak handle to get the App, then cast the ModelRc back to VecModel
        // via the stored Rc wrapped in Arc<Mutex<Option<...>>> using a Send wrapper.

        // We'll use a wrapper to make Rc<VecModel<T>> Send-able for this one specific purpose.
        // This is safe because we only access it from the Slint event loop thread.
        struct SendRcModel<T: 'static + Clone>(Rc<slint::VecModel<T>>);
        // Safety: we only ever use these from within invoke_from_event_loop,
        // which executes on the Slint UI thread where the Rc was created.
        unsafe impl<T: 'static + Clone> Send for SendRcModel<T> {}

        let input_model_send = Arc::new(Mutex::new(SendRcModel(input_model)));

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
                let input_model_send = Arc::clone(&input_model_send);

                let ok = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };

                    // Pause the live preview while the window is unfocused to
                    // save resources; the overlay tells the user it's intentional.
                    let focused = ui
                        .window()
                        .with_winit_window(|w| w.has_focus())
                        .unwrap_or(true);
                    ui.set_preview_paused(src_on && !focused);

                    if src_on {
                        // Panels + point cloud are driven by the dedicated render
                        // timer; this slow poll only maintains connection status.
                        if src_rate > 0.0 {
                            ui.set_source_status(format!("{src_rate:.1} packets/s").into());
                            ui.set_source_status_color(color(COLOR_GREEN));
                            src_had.store(true, Ordering::Relaxed);
                        } else if src_had.load(Ordering::Relaxed) {
                            ui.set_source_status("Reconnecting...".into());
                            ui.set_source_status_color(color(COLOR_ORANGE));
                        }
                    } else {
                        if src_had.load(Ordering::Relaxed) {
                            // Source just disconnected — reset input values to 0
                            // and settle the panels + mesh to that pose (the
                            // render timer is idle while disconnected).
                            let input_model = &input_model_send.lock().unwrap().0;
                            for i in 0..input_model.row_count() {
                                if let Some(mut pair) = input_model.row_data(i) {
                                    pair.value = 0.0;
                                    input_model.set_row_data(i, pair);
                                }
                            }
                            let output = eval_outputs(&collect_input_values(input_model), false);
                            update_outputs_and_mesh(&ui, &output);
                        }
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

    // Close guard — intercept window close via the Rust Window API
    {
        let weak = app.as_weak();
        let source_active = Arc::clone(&source_active);
        let target_active = Arc::clone(&target_active);
        app.window().on_close_requested(move || {
            let Some(ui) = weak.upgrade() else {
                return slint::CloseRequestResponse::HideWindow;
            };

            let has_unsaved = ui.get_has_unsaved_changes();
            let is_connected = source_active.load(Ordering::Relaxed)
                || target_active.load(Ordering::Relaxed);

            if has_unsaved {
                let dialog = UnsavedChangesDialog::new().unwrap();

                {
                    let dialog_weak = dialog.as_weak();
                    let ui_weak = ui.as_weak();
                    dialog.on_do_save(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            let snek = build_snek_from_editor(&ui);
                            let _ = preset::save_preset(&presets_dir(), &snek);
                        }
                        if let Some(dlg) = dialog_weak.upgrade() {
                            let _ = dlg.hide();
                        }
                        let _ = slint::quit_event_loop();
                    });
                }

                {
                    let dialog_weak = dialog.as_weak();
                    dialog.on_do_discard(move || {
                        if let Some(dlg) = dialog_weak.upgrade() {
                            let _ = dlg.hide();
                        }
                        let _ = slint::quit_event_loop();
                    });
                }

                {
                    let dialog_weak = dialog.as_weak();
                    dialog.on_do_cancel(move || {
                        if let Some(dlg) = dialog_weak.upgrade() {
                            let _ = dlg.hide();
                        }
                    });
                }

                dialog.show().unwrap();
                slint::CloseRequestResponse::KeepWindowShown
            } else if is_connected {
                let dialog = ActiveConnectionDialog::new().unwrap();

                {
                    let dialog_weak = dialog.as_weak();
                    dialog.on_do_close(move || {
                        if let Some(dlg) = dialog_weak.upgrade() {
                            let _ = dlg.hide();
                        }
                        let _ = slint::quit_event_loop();
                    });
                }

                {
                    let dialog_weak = dialog.as_weak();
                    dialog.on_do_cancel(move || {
                        if let Some(dlg) = dialog_weak.upgrade() {
                            let _ = dlg.hide();
                        }
                    });
                }

                dialog.show().unwrap();
                slint::CloseRequestResponse::KeepWindowShown
            } else {
                slint::CloseRequestResponse::HideWindow
            }
        });
    }

    // Dedicated ~30fps render loop for the face point cloud, decoupled from the
    // slow status/panel poll. Evaluates the cached preview state against the
    // latest tracking values, steps the delay buffers, and re-projects the mesh.
    // Idle unless the preview tab is visible, the window is focused, and a
    // source is live, so it costs nothing when not in use.
    let cloud_timer = slint::Timer::default();
    {
        let weak = app.as_weak();
        let live_values = Arc::clone(&live_values);
        let source_active = Arc::clone(&source_active);
        let input_model = Rc::clone(&input_model);
        cloud_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(33),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                if ui.get_active_tab() != 0 || !source_active.load(Ordering::Relaxed) {
                    return;
                }
                let focused = ui
                    .window()
                    .with_winit_window(|w| w.has_focus())
                    .unwrap_or(true);
                if !focused {
                    return;
                }
                // Refresh input panel, output panel (value meters) and the point
                // cloud together, all in place, at the render frame rate.
                let values = live_values.lock().unwrap().clone();
                update_input_model(&input_model, &values);
                let output = eval_outputs(&collect_input_values(&input_model), true);
                update_outputs_and_mesh(&ui, &output);
            },
        );
    }

    app.run().unwrap();

    // Signal all background tasks to stop on window close
    source_active.store(false, Ordering::Relaxed);
    target_active.store(false, Ordering::Relaxed);

    // Drop the runtime, which waits for blocking tasks to finish
    drop(rt);
}
