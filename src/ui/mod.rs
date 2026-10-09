//! Desktop commands and the tracking update loop, independent of the web renderer.
mod runtime;

#[cfg(test)]
mod tests;

use std::{sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::{model::Preset, presets, settings::Settings};
#[cfg(test)]
pub(crate) use runtime::face_present_after_timeout;
use runtime::{Runtime, Snapshot};

pub struct Desktop(pub Mutex<Runtime>);

fn with_runtime<T>(
    desktop: &Desktop,
    action: impl FnOnce(&mut Runtime) -> Result<T, String>,
) -> Result<T, String> {
    let mut runtime = desktop
        .0
        .lock()
        .map_err(|_| "Application state is unavailable")?;
    action(&mut runtime)
}

#[tauri::command]
fn snapshot(desktop: State<'_, Desktop>) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| Ok(r.snapshot()))
}

#[tauri::command]
fn update_settings(desktop: State<'_, Desktop>, settings: Settings) -> Result<(), String> {
    with_runtime(&desktop, |r| {
        r.settings = settings;
        r.persist();
        Ok(())
    })
}

#[tauri::command]
fn update_preset(desktop: State<'_, Desktop>, preset: Preset) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        if preset.format != "snek" || preset.version != 1 {
            return Err("Unsupported preset format".into());
        }
        r.preset = preset;
        r.dirty = true;
        r.rebuild();
        Ok(r.snapshot())
    })
}

#[tauri::command]
fn save_preset(desktop: State<'_, Desktop>) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        r.save()?;
        Ok(r.snapshot())
    })
}

#[tauri::command]
fn switch_preset(
    desktop: State<'_, Desktop>,
    index: usize,
    discard: bool,
) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        if r.dirty && !discard {
            return Err("Save or discard unsaved changes first.".into());
        }
        let choice = r.choices.get(index).ok_or("Preset not found")?.clone();
        r.selected = index;
        r.stored_selection = choice.filename.is_some();
        r.preset = choice.preset;
        r.dirty = false;
        r.rebuild();
        r.persist();
        Ok(r.snapshot())
    })
}

#[tauri::command]
fn new_preset(
    desktop: State<'_, Desktop>,
    title: String,
    author: String,
    description: String,
    base: Option<usize>,
    discard: bool,
) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        if r.dirty && !discard {
            return Err("Save or discard unsaved changes first.".into());
        }
        let params = match base {
            Some(index) => r
                .choices
                .get(index)
                .ok_or("Base preset not found")?
                .preset
                .params
                .clone(),
            None => vec![],
        };
        let mut preset = Preset::new(title, params);
        preset.author = author;
        preset.description = description;
        r.add(preset);
        Ok(r.snapshot())
    })
}

#[tauri::command]
fn delete_preset(desktop: State<'_, Desktop>, discard: bool) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        if r.dirty && !discard {
            return Err("Save or discard unsaved changes first.".into());
        }
        let filename = r
            .choices
            .get(r.selected)
            .and_then(|c| c.filename.clone())
            .ok_or("Only saved presets can be deleted")?;
        r.store.delete(&filename)?;
        r.choices.remove(r.selected);
        r.selected = 0;
        r.preset = r.choices[0].preset.clone();
        r.stored_selection = r.choices[0].filename.is_some();
        r.dirty = false;
        r.rebuild();
        r.persist();
        Ok(r.snapshot())
    })
}

#[derive(Serialize)]
struct ImportPreview {
    preset: Preset,
    vitamins: bool,
}

#[tauri::command]
fn pick_import(desktop: State<'_, Desktop>) -> Result<Option<ImportPreview>, String> {
    with_runtime(&desktop, |r| r.ensure_editable())?;
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Preset", &["snek", "json", "vps"])
        .pick_file()
    else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not read preset: {e}"))?;
    let vitamins = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vps"));
    let preset = if vitamins {
        presets::import_vitamins(&text, false)?.preset
    } else {
        presets::parse(&text)?
    };
    with_runtime(&desktop, |r| {
        r.import_text = Some((text, vitamins));
        Ok(())
    })?;
    Ok(Some(ImportPreview { preset, vitamins }))
}

#[tauri::command]
fn import_preset(
    desktop: State<'_, Desktop>,
    title: String,
    author: String,
    description: String,
    swap_xy: bool,
    discard: bool,
) -> Result<Snapshot, String> {
    with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        if r.dirty && !discard {
            return Err("Save or discard unsaved changes first.".into());
        }
        let (text, vitamins) = r.import_text.as_ref().ok_or("Choose a preset file first")?;
        let (mut preset, warnings) = if *vitamins {
            let result = presets::import_vitamins(text, swap_xy)?;
            (result.preset, result.warnings)
        } else {
            (presets::parse(text)?, vec![])
        };
        preset.title = title;
        preset.author = author;
        preset.description = description;
        r.add(preset);
        r.import_text = None;
        if !warnings.is_empty() {
            r.error = warnings.join(" · ");
        }
        Ok(r.snapshot())
    })
}

#[tauri::command]
fn export_preset(desktop: State<'_, Desktop>) -> Result<(), String> {
    let text = with_runtime(&desktop, |r| {
        r.ensure_editable()?;
        serde_json::to_string_pretty(&r.preset).map_err(|e| e.to_string())
    })?;
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Snenk preset", &["snek"])
        .set_file_name("preset.snek")
        .save_file()
    {
        std::fs::write(path, text).map_err(|e| format!("Could not export: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
fn toggle_source(desktop: State<'_, Desktop>) -> Result<(), String> {
    with_runtime(&desktop, Runtime::toggle_source)
}

#[tauri::command]
fn toggle_target(desktop: State<'_, Desktop>) -> Result<(), String> {
    with_runtime(&desktop, Runtime::toggle_target)
}

#[tauri::command]
fn manual_input(desktop: State<'_, Desktop>, name: String, value: f64) -> Result<(), String> {
    with_runtime(&desktop, |r| {
        if r.source.is_some() {
            return Err("Disconnect tracking before changing manual inputs.".into());
        }
        if name.trim().is_empty() || !value.is_finite() {
            return Err("Enter a variable name and a finite number.".into());
        }
        r.manual.insert(name, value);
        r.evaluate(false);
        Ok(())
    })
}

#[tauri::command]
fn advance_preview(desktop: State<'_, Desktop>) -> Result<(), String> {
    with_runtime(&desktop, |r| {
        if r.source.is_some() {
            return Err("Disconnect tracking before advancing manually.".into());
        }
        r.evaluate(true);
        Ok(())
    })
}

#[tauri::command]
fn close_app(app: tauri::AppHandle, desktop: State<'_, Desktop>) -> Result<(), String> {
    with_runtime(&desktop, |r| {
        r.persist();
        r.source = None;
        r.target = None;
        Ok(())
    })?;
    app.exit(0);
    Ok(())
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .manage(Desktop(Mutex::new(Runtime::new())))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            update_settings,
            update_preset,
            save_preset,
            switch_preset,
            new_preset,
            delete_preset,
            pick_import,
            import_preset,
            export_preset,
            toggle_source,
            toggle_target,
            manual_input,
            advance_preview,
            close_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(16));
                let frame = with_runtime(handle.state::<Desktop>().inner(), |r| Ok(r.tick()));
                match frame {
                    Ok(Some(frame)) => {
                        if let Err(e) = handle.emit("preview-update", frame) {
                            log::warn!("Could not send preview update: {e}");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::error!("Tracking loop stopped: {e}");
                        break;
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.emit("close-requested", ()) {
                    log::warn!("Could not request unsaved-change confirmation: {e}");
                }
            }
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested {
                api, code: None, ..
            } = &event
            {
                api.prevent_exit();
                if let Err(e) = app.emit("close-requested", ()) {
                    log::warn!("Could not request close confirmation: {e}");
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                let _ = with_runtime(app.state::<Desktop>().inner(), |r| {
                    r.persist();
                    r.source = None;
                    r.target = None;
                    Ok(())
                });
            }
        });
    Ok(())
}
