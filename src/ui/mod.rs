mod actions;
mod convert;
mod preview;
mod types;

#[allow(unused_imports)]
pub(crate) use convert::face_present_after_timeout;

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use slint::{ComponentHandle, Timer, TimerMode, VecModel};

use crate::evaluation::Evaluator;
use crate::model::{DelaySettings, Parameter, Preset};
use crate::network::{SourceHandle, SourceKind, SourceSettings, TargetHandle, TargetSettings};
use crate::presets::{self, PresetStore};
use crate::settings;
use crate::{
    AboutWindow, App, ImportDialog, NewPresetDialog, UnsavedChangesDialog, VarEntry,
    VariablesWindow, DEPENDENCY_CREDITS,
};

use actions::Ui;
use convert::parameter;
use types::{builtins, input_defaults, Action, Choice, State};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::new()?;
    let saved = settings::load_settings();
    app.set_phone_ip(saved.phone_ip.into());
    app.set_tracking_type_index(saved.tracking_type_index);
    app.set_face_search_timeout(saved.face_search_timeout.into());
    app.set_vts_ip(saved.vts_ip.into());
    app.set_vts_port(saved.vts_port.into());
    let store = PresetStore::new(settings::app_dir().join("presets"));
    let mut choices = builtins();
    match store.list() {
        Ok(stored) => choices.extend(stored.into_iter().map(|c| Choice {
            preset: c.preset,
            filename: Some(c.filename),
        })),
        Err(e) => app.set_error_text(e.into()),
    }
    if choices.is_empty() {
        choices.push(Choice {
            preset: Preset::new("Default", vec![]),
            filename: None,
        });
    }
    let selected = choices
        .iter()
        .position(|c| c.preset.title == saved.preset_name)
        .unwrap_or(0);
    let stored_selection = choices[selected].filename.is_some();
    let preset = choices[selected].preset.clone();
    let ui = Rc::new(Ui {
        app: app.as_weak(),
        state: RefCell::new(State {
            evaluator: Evaluator::new(&preset.params).ok(),
            preset,
            choices,
            selected,
            store,
            source: None,
            target: None,
            manual: input_defaults(),
            frame: None,
            frame_count: 0,
            last_frame: None,
            rate_at: Instant::now(),
            rate_count: 0,
            rate: 0.0,
            session: Instant::now(),
            pending: None,
            import_text: None,
            stored_selection,
            preview_dirty: true,
            last_preview_face: None,
            last_outputs: None,
        }),
        new: NewPresetDialog::new()?,
        import: ImportDialog::new()?,
        guard: UnsavedChangesDialog::new()?,
        variables: VariablesWindow::new()?,
    });
    ui.sync();
    ui.rebuild();
    macro_rules! bind {
        ($object:expr, $handler:ident, |$u:ident $(, $arg:ident)*| $body:block) => {{
            let weak = Rc::downgrade(&ui);
            $object.$handler(move |$($arg),*| {
                if let Some($u) = weak.upgrade() {
                    $body
                }
            });
        }};
    }
    bind!(app, on_editor_save, |u| {
        u.save();
    });
    bind!(app, on_settings_changed, |u| {
        if let Some(a) = u.app.upgrade() {
            let index = a.get_preset_index().max(0) as usize;
            if index != u.state.borrow().selected {
                u.request(Action::Switch(index));
            } else {
                u.persist();
            }
        }
    });
    bind!(app, on_editor_metadata_changed, |u| {
        if let Some(a) = u.app.upgrade() {
            let mut s = u.state.borrow_mut();
            s.preset.title = a.get_editor_title().to_string();
            s.preset.author = a.get_editor_author().to_string();
            s.preset.description = a.get_editor_description().to_string();
            drop(s);
            u.dirty();
        }
    });
    bind!(app, on_editor_param_changed, |u, index, p| {
        if u.state.borrow().target.is_some() {
            u.error("Stop VTube Studio output before editing parameters.");
            u.sync();
            return;
        }
        if index >= 0 {
            if let Some(old) = u.state.borrow_mut().preset.params.get_mut(index as usize) {
                *old = parameter(p);
            }
            u.dirty();
        }
    });
    bind!(app, on_editor_delete_param, |u, index| {
        if u.state.borrow().target.is_some() {
            u.error("Stop VTube Studio output before editing parameters.");
            return;
        }
        let mut s = u.state.borrow_mut();
        if index >= 0 && (index as usize) < s.preset.params.len() {
            s.preset.params.remove(index as usize);
        }
        drop(s);
        u.sync();
        u.dirty();
    });
    for delay in [false, true] {
        let weak = Rc::downgrade(&ui);
        let callback = move || {
            if let Some(u) = weak.upgrade() {
                let mut s = u.state.borrow_mut();
                if s.target.is_some() {
                    u.error("Stop VTube Studio output before editing parameters.");
                    return;
                }
                let mut n = s.preset.params.len() + 1;
                while s
                    .preset
                    .params
                    .iter()
                    .any(|p| p.name == format!("Parameter{n}"))
                {
                    n += 1;
                }
                let reference = s
                    .preset
                    .params
                    .first()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "JawOpen".into());
                s.preset.params.push(Parameter {
                    name: format!("Parameter{n}"),
                    func: if delay {
                        String::new()
                    } else {
                        "JawOpen".into()
                    },
                    min: 0.0,
                    max: 1.0,
                    default_value: 0.0,
                    delay_buffer: delay.then_some(DelaySettings {
                        ref_param: reference,
                        smoothing: 1.0,
                        delay_count: 1,
                        in_min: 0.0,
                        in_max: 1.0,
                        out_min: 0.0,
                        out_max: 1.0,
                    }),
                });
                drop(s);
                u.sync();
                u.dirty();
            }
        };
        if delay {
            app.on_editor_add_delay_param(callback);
        } else {
            app.on_editor_add_param(callback);
        }
    }
    bind!(app, on_new_preset, |u| {
        u.request(Action::New);
    });
    bind!(app, on_import_preset, |u| {
        u.request(Action::Import);
    });
    bind!(app, on_delete_preset, |u| {
        u.request(Action::Delete);
    });
    bind!(app, on_export_preset, |u| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Snenk preset", &["snek"])
            .set_file_name("preset.snek")
            .save_file()
        {
            let result = serde_json::to_string_pretty(&u.state.borrow().preset)
                .map_err(|e| e.to_string())
                .and_then(|text| std::fs::write(path, text).map_err(|e| e.to_string()));
            if let Err(e) = result {
                u.error(format!("Could not export: {e}"));
            }
        }
    });
    bind!(ui.new, on_do_create, |u| {
        let mut s = u.state.borrow_mut();
        let index = u.new.get_base_preset_index();
        let params = if index > 0 {
            s.choices
                .get((index - 1) as usize)
                .map(|c| c.preset.params.clone())
                .unwrap_or_default()
        } else {
            vec![]
        };
        let mut p = Preset::new(u.new.get_preset_title().to_string(), params);
        p.author = u.new.get_preset_author().to_string();
        p.description = u.new.get_preset_description().to_string();
        s.preset = p;
        let transient = s.preset.clone();
        s.choices.push(Choice {
            preset: transient,
            filename: None,
        });
        s.selected = s.choices.len() - 1;
        s.stored_selection = false;
        drop(s);
        let _ = u.new.hide();
        u.sync();
        u.dirty();
    });
    bind!(ui.new, on_do_cancel, |u| {
        let _ = u.new.hide();
    });
    bind!(ui.import, on_do_import, |u| {
        let data = u.state.borrow().import_text.clone();
        if let Some((text, vitamins)) = data {
            let result = if vitamins {
                presets::import_vitamins(&text, u.import.get_swap_xy())
                    .map(|r| (r.preset, r.warnings))
            } else {
                presets::parse(&text).map(|p| (p, vec![]))
            };
            match result {
                Ok((mut p, warnings)) => {
                    p.title = u.import.get_preset_title().to_string();
                    p.author = u.import.get_preset_author().to_string();
                    p.description = u.import.get_preset_description().to_string();
                    let mut state = u.state.borrow_mut();
                    state.preset = p;
                    let transient = state.preset.clone();
                    state.choices.push(Choice {
                        preset: transient,
                        filename: None,
                    });
                    state.selected = state.choices.len() - 1;
                    state.stored_selection = false;
                    drop(state);
                    let _ = u.import.hide();
                    u.sync();
                    u.dirty();
                    if !warnings.is_empty() {
                        u.error(warnings.join(" · "));
                    }
                }
                Err(e) => u.error(e),
            }
        }
    });
    bind!(ui.import, on_do_cancel, |u| {
        let _ = u.import.hide();
        u.state.borrow_mut().import_text = None;
    });
    bind!(ui.guard, on_do_save, |u| {
        u.finish_guard(true);
    });
    bind!(ui.guard, on_do_discard, |u| {
        u.finish_guard(false);
    });
    bind!(ui.guard, on_do_cancel, |u| {
        u.state.borrow_mut().pending = None;
        let _ = u.guard.hide();
    });
    bind!(app, on_manual_input, |u, name, value| {
        match value.parse::<f64>() {
            Ok(v) if v.is_finite() => {
                if name.trim().is_empty() {
                    u.error("Enter a variable name.");
                } else {
                    u.state.borrow_mut().manual.insert(name.to_string(), v);
                    u.error("");
                    u.preview(false);
                }
            }
            _ => u.error("Enter a finite number."),
        }
    });
    bind!(app, on_advance_preview, |u| {
        u.preview(true);
    });
    bind!(app, on_toggle_source, |u| {
        let Some(a) = u.app.upgrade() else { return };
        let mut s = u.state.borrow_mut();
        if s.source.take().is_some() {
            if let Some(target) = &s.target {
                target.publish(s.last_outputs.clone().unwrap_or_default(), false);
            }
            s.last_preview_face = Some(false);
            a.set_source_active(false);
            a.set_source_status("Stopped".into());
        } else {
            if a.get_face_search_timeout().parse::<u64>().is_err() {
                u.error("Face-loss timeout must be a nonnegative number of milliseconds.");
                return;
            }
            s.frame = None;
            s.last_frame = None;
            s.frame_count = 0;
            s.rate_count = 0;
            s.rate_at = Instant::now();
            s.source = Some(SourceHandle::start(SourceSettings {
                kind: if a.get_tracking_type_index() == 1 {
                    SourceKind::IFacialMocap
                } else {
                    SourceKind::VTubeStudio
                },
                phone_address: a.get_phone_ip().to_string(),
            }));
            a.set_source_active(true);
        }
        drop(s);
        u.persist();
    });
    bind!(app, on_toggle_target, |u| {
        let Some(a) = u.app.upgrade() else { return };
        let mut s = u.state.borrow_mut();
        if s.target.take().is_some() {
            a.set_target_active(false);
            a.set_target_status("Stopped".into());
        } else {
            let Ok(port) = a.get_vts_port().parse::<u16>() else {
                u.error("VTube Studio port must be between 1 and 65535.");
                return;
            };
            if port == 0 {
                u.error("VTube Studio port must be between 1 and 65535.");
                return;
            }
            if s.evaluator.is_none() {
                u.error("Fix preset validation errors before starting output.");
                return;
            }
            s.target = Some(TargetHandle::start(
                TargetSettings {
                    host: a.get_vts_ip().to_string(),
                    port,
                    token_path: settings::app_dir().join("vts-token.json"),
                },
                s.preset.params.clone(),
            ));
            s.preview_dirty = true;
            a.set_target_active(true);
        }
        drop(s);
        u.persist();
    });
    bind!(app, on_editor_show_variables, |u| {
        let mut entries = input_defaults()
            .keys()
            .map(|n| VarEntry {
                name: n.clone().into(),
                note: if n.starts_with("HeadRot") {
                    "Degrees".into()
                } else if n == "FaceFound" {
                    "0 or 1".into()
                } else {
                    "Tracking input".into()
                },
                header: false,
            })
            .collect::<Vec<_>>();
        entries.extend(
            [
                ("WaveN", "Triangle 0 → 1 → 0; period N milliseconds"),
                ("PingPongN", "Ramp 0 → 1; period N milliseconds"),
            ]
            .into_iter()
            .map(|(n, note)| VarEntry {
                name: n.into(),
                note: note.into(),
                header: false,
            }),
        );
        u.variables
            .set_entries(Rc::new(VecModel::from(entries)).into());
        let _ = u.variables.show();
    });
    let about = AboutWindow::new()?;
    about.set_credits_text(DEPENDENCY_CREDITS.into());
    let about_weak = about.as_weak();
    app.on_show_about(move || {
        if let Some(a) = about_weak.upgrade() {
            let _ = a.show();
        }
    });
    let weak = Rc::downgrade(&ui);
    app.window().on_close_requested(move || {
        if let Some(u) = weak.upgrade() {
            u.request(Action::Close);
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            slint::CloseRequestResponse::HideWindow
        }
    });
    let timer = Timer::default();
    let timer_weak = Rc::downgrade(&ui);
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(u) = timer_weak.upgrade() {
            u.tick();
        }
    });
    app.run()?;
    Ok(())
}
