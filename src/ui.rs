use crate::evaluation::{self, Evaluator};
use crate::model::{DelaySettings, Parameter, Preset, TrackingFrame};
use crate::network::{
    NetworkStatus, SourceHandle, SourceKind, SourceSettings, TargetHandle, TargetSettings,
};
use crate::presets::{self, PresetStore};
use crate::{
    face_mesh, settings, AboutWindow, App, EditorParam, ImportDialog, NameValuePair,
    NewPresetDialog, UnsavedChangesDialog, VarEntry, VariablesWindow, BLENDSHAPE_NAMES,
    DEPENDENCY_CREDITS,
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Choice {
    preset: Preset,
    filename: Option<String>,
}
#[derive(Clone)]
enum Action {
    Switch(usize),
    New,
    Import,
    Delete,
    Close,
}
struct State {
    choices: Vec<Choice>,
    selected: usize,
    preset: Preset,
    store: PresetStore,
    evaluator: Option<Evaluator>,
    source: Option<SourceHandle>,
    target: Option<TargetHandle>,
    manual: BTreeMap<String, f64>,
    frame: Option<TrackingFrame>,
    frame_count: u64,
    last_frame: Option<Instant>,
    rate_at: Instant,
    rate_count: u64,
    rate: f64,
    session: Instant,
    pending: Option<Action>,
    import_text: Option<(String, bool)>,
    stored_selection: bool,
    preview_dirty: bool,
    last_preview_face: Option<bool>,
    last_outputs: Option<BTreeMap<String, f64>>,
}
struct Ui {
    app: slint::Weak<App>,
    state: RefCell<State>,
    new: NewPresetDialog,
    import: ImportDialog,
    guard: UnsavedChangesDialog,
    variables: VariablesWindow,
}
fn strings(values: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    Rc::new(VecModel::from(
        values.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
    .into()
}
fn row(p: &Parameter) -> EditorParam {
    let d = p.delay_buffer.clone().unwrap_or(DelaySettings {
        ref_param: String::new(),
        smoothing: 1.0,
        delay_count: 1,
        in_min: 0.0,
        in_max: 1.0,
        out_min: 0.0,
        out_max: 1.0,
    });
    EditorParam {
        name: p.name.clone().into(),
        func: p.func.clone().into(),
        min: p.min as f32,
        max: p.max as f32,
        default_value: p.default_value as f32,
        is_delay: p.delay_buffer.is_some(),
        ref_param: d.ref_param.into(),
        smoothing: d.smoothing as f32,
        delay_count: d.delay_count as f32,
        in_min: d.in_min as f32,
        in_max: d.in_max as f32,
        out_min: d.out_min as f32,
        out_max: d.out_max as f32,
    }
}
fn parameter(p: EditorParam) -> Parameter {
    Parameter {
        name: p.name.to_string(),
        func: p.func.to_string(),
        min: p.min as f64,
        max: p.max as f64,
        default_value: p.default_value as f64,
        delay_buffer: p.is_delay.then(|| DelaySettings {
            ref_param: p.ref_param.to_string(),
            smoothing: p.smoothing as f64,
            delay_count: p.delay_count as usize,
            in_min: p.in_min as f64,
            in_max: p.in_max as f64,
            out_min: p.out_min as f64,
            out_max: p.out_max as f64,
        }),
    }
}
fn input_defaults() -> BTreeMap<String, f64> {
    BLENDSHAPE_NAMES
        .iter()
        .copied()
        .chain([
            "HeadPosX",
            "HeadPosY",
            "HeadPosZ",
            "HeadRotX",
            "HeadRotY",
            "HeadRotZ",
            "FaceFound",
        ])
        .map(|n| (n.into(), 0.0))
        .collect()
}
fn builtins() -> Vec<Choice> {
    [
        ("Default", include_str!("../presets/default.json")),
        (
            "Maruseu Enhanced",
            include_str!("../presets/maruseu_enhanced.json"),
        ),
        (
            "Maruseu VBridger",
            include_str!("../presets/maruseu_vbridger.json"),
        ),
    ]
    .into_iter()
    .filter_map(|(title, text)| match presets::parse(text) {
        Ok(mut preset) => {
            if preset.title.is_empty() {
                preset.title = title.into();
            }
            Some(Choice {
                preset,
                filename: None,
            })
        }
        Err(e) => {
            log::error!("Bundled preset {title}: {e}");
            None
        }
    })
    .collect()
}
impl Ui {
    fn error(&self, message: impl Into<SharedString>) {
        if let Some(app) = self.app.upgrade() {
            app.set_error_text(message.into());
        }
    }
    fn persist(&self) {
        if let Some(a) = self.app.upgrade() {
            settings::save_settings(&settings::Settings {
                preset_name: self.state.borrow().preset.title.clone(),
                phone_ip: a.get_phone_ip().to_string(),
                tracking_type_index: a.get_tracking_type_index(),
                face_search_timeout: a.get_face_search_timeout().to_string(),
                vts_ip: a.get_vts_ip().to_string(),
                vts_port: a.get_vts_port().to_string(),
            });
        }
    }
    fn sync(&self) {
        let Some(a) = self.app.upgrade() else { return };
        let s = self.state.borrow();
        a.set_preset_names(strings(s.choices.iter().map(|c| c.preset.title.clone())));
        a.set_preset_index(s.selected as i32);
        a.set_can_delete_preset(s.stored_selection);
        a.set_editor_title(s.preset.title.clone().into());
        a.set_editor_author(s.preset.author.clone().into());
        a.set_editor_description(s.preset.description.clone().into());
        a.set_editor_params(
            Rc::new(VecModel::from(
                s.preset.params.iter().map(row).collect::<Vec<_>>(),
            ))
            .into(),
        );
        a.set_editor_param_errors(strings(evaluation::validation_errors(&s.preset.params)));
    }
    fn rebuild(&self) {
        let mut s = self.state.borrow_mut();
        match Evaluator::new(&s.preset.params) {
            Ok(e) => {
                s.evaluator = Some(e);
                self.error("");
            }
            Err(e) => {
                s.evaluator = None;
                self.error(e);
            }
        }
        if let Some(a) = self.app.upgrade() {
            a.set_editor_param_errors(strings(evaluation::validation_errors(&s.preset.params)));
        }
        drop(s);
        self.preview(false);
    }
    fn dirty(&self) {
        if let Some(a) = self.app.upgrade() {
            a.set_has_unsaved_changes(true);
        }
        self.state.borrow_mut().preview_dirty = true;
        self.rebuild();
    }
    fn save(&self) -> bool {
        let mut s = self.state.borrow_mut();
        if s.preset.title.trim().is_empty() {
            self.error("A preset needs a title.");
            return false;
        }
        match s.store.save(&s.preset) {
            Ok(filename) => {
                let choice = Choice {
                    preset: s.preset.clone(),
                    filename: Some(filename.clone()),
                };
                if !s.stored_selection && s.selected < s.choices.len() {
                    let selected = s.selected;
                    s.choices[selected] = choice;
                } else if let Some(i) = s
                    .choices
                    .iter()
                    .position(|c| c.filename.as_ref() == Some(&filename))
                {
                    s.choices[i] = choice;
                    s.selected = i;
                } else {
                    s.choices.push(choice);
                    s.selected = s.choices.len() - 1;
                }
                s.stored_selection = true;
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.persist();
                self.error("");
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }
    fn request(&self, action: Action) {
        let Some(a) = self.app.upgrade() else { return };
        if a.get_has_unsaved_changes() {
            self.state.borrow_mut().pending = Some(action);
            a.set_preset_index(self.state.borrow().selected as i32);
            if let Err(e) = self.guard.show() {
                self.error(e.to_string());
            }
        } else {
            self.execute(action);
        }
    }
    fn execute(&self, action: Action) {
        match action {
            Action::Switch(index) => {
                let mut s = self.state.borrow_mut();
                if let Some(c) = s.choices.get(index).cloned() {
                    s.selected = index;
                    s.preset = c.preset;
                    s.stored_selection = c.filename.is_some();
                }
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.rebuild();
                self.persist();
            }
            Action::New => {
                self.new.set_preset_title("New preset".into());
                self.new.set_preset_author("".into());
                self.new.set_preset_description("".into());
                self.new.set_base_preset_names(strings(
                    std::iter::once("Empty".into()).chain(
                        self.state
                            .borrow()
                            .choices
                            .iter()
                            .map(|c| c.preset.title.clone()),
                    ),
                ));
                self.new.set_base_preset_index(0);
                let _ = self.new.show();
            }
            Action::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Preset", &["snek", "json", "vps"])
                    .pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            let vitamins = path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("vps"));
                            let parsed = if vitamins {
                                presets::import_vitamins(&text, false).map(|r| r.preset)
                            } else {
                                presets::parse(&text)
                            };
                            match parsed {
                                Ok(p) => {
                                    self.import.set_preset_title(p.title.into());
                                    self.import.set_preset_author(p.author.into());
                                    self.import.set_preset_description(p.description.into());
                                    self.import.set_show_swap_toggle(vitamins);
                                    self.import.set_swap_xy(false);
                                    self.state.borrow_mut().import_text = Some((text, vitamins));
                                    let _ = self.import.show();
                                }
                                Err(e) => self.error(e),
                            }
                        }
                        Err(e) => self.error(format!("Could not read preset: {e}")),
                    }
                }
            }
            Action::Delete => {
                let mut s = self.state.borrow_mut();
                if !s.stored_selection {
                    return;
                }
                if let Some(filename) = s.choices.get(s.selected).and_then(|c| c.filename.clone()) {
                    match s.store.delete(&filename) {
                        Ok(()) => {
                            let index = s.selected;
                            s.choices.remove(index);
                            s.selected = 0;
                            s.preset = s.choices[0].preset.clone();
                            s.stored_selection = s.choices[0].filename.is_some();
                        }
                        Err(e) => {
                            self.error(e);
                            return;
                        }
                    }
                }
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.rebuild();
                self.persist();
            }
            Action::Close => {
                self.persist();
                let mut s = self.state.borrow_mut();
                s.source = None;
                s.target = None;
                drop(s);
                let _ = slint::quit_event_loop();
            }
        }
    }
    fn finish_guard(&self, save: bool) {
        if save && !self.save() {
            return;
        }
        let _ = self.guard.hide();
        let action = self.state.borrow_mut().pending.take();
        if let Some(action) = action {
            if let Some(a) = self.app.upgrade() {
                a.set_has_unsaved_changes(false);
            }
            self.execute(action);
        }
    }
    fn preview(&self, advance: bool) {
        let Some(a) = self.app.upgrade() else { return };
        let mut s = self.state.borrow_mut();
        let live = s.source.is_some();
        let mut values = if live {
            s.frame
                .as_ref()
                .map(|f| f.values.clone())
                .unwrap_or_else(input_defaults)
        } else {
            s.manual.clone()
        };
        let timeout = a.get_face_search_timeout().parse::<u64>().unwrap_or(3000);
        let face = if live {
            face_present_after_timeout(
                s.frame.as_ref().is_some_and(|f| f.face_present),
                s.last_frame,
                Duration::from_millis(timeout),
            )
        } else {
            values.get("FaceFound").copied().unwrap_or(0.0) > 0.0
        };
        values.insert("FaceFound".into(), f64::from(face));
        values.extend(evaluation::time_variables(
            &s.preset.params,
            s.session.elapsed().as_millis() as u64,
        ));
        let outputs = s
            .evaluator
            .as_mut()
            .map(|e| e.evaluate(&values, advance))
            .unwrap_or_default();
        s.last_outputs = Some(outputs.clone());
        s.last_preview_face = Some(face);
        if live && s.frame.is_some() {
            if let Some(target) = &s.target {
                target.publish(outputs.clone(), face);
            }
        }
        a.set_input_shapes(
            Rc::new(VecModel::from(
                values
                    .iter()
                    .map(|(name, value)| NameValuePair {
                        name: name.clone().into(),
                        value: *value as f32,
                        min: if name.starts_with("Head") {
                            -180.0
                        } else {
                            0.0
                        },
                        max: if name.starts_with("Head") { 180.0 } else { 1.0 },
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        a.set_output_params(
            Rc::new(VecModel::from(
                s.preset
                    .params
                    .iter()
                    .filter_map(|p| {
                        outputs.get(&p.name).map(|value| NameValuePair {
                            name: p.name.clone().into(),
                            value: *value as f32,
                            min: p.min as f32,
                            max: p.max as f32,
                        })
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        a.set_mesh_image(face_mesh::compute_input_preview(|name| {
            values.get(name).map(|v| *v as f32)
        }));
        s.preview_dirty = false;
    }
    fn tick(&self) {
        let Some(a) = self.app.upgrade() else { return };
        let mut s = self.state.borrow_mut();
        let mut frame_changed = false;
        if let Some(source) = &s.source {
            let (status, frame, count) = source.snapshot();
            if count != s.frame_count {
                s.frame_count = count;
                s.frame = frame;
                s.last_frame = Some(Instant::now());
                frame_changed = true;
            }
            let elapsed = s.rate_at.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                s.rate = count.saturating_sub(s.rate_count) as f64 / elapsed;
                s.rate_count = count;
                s.rate_at = Instant::now();
            }
            let text = match status {
                NetworkStatus::Connected => {
                    if s.last_frame
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                    {
                        format!("Receiving · {:.0} fps", s.rate)
                    } else if s.frame.is_some() {
                        "Interrupted".into()
                    } else {
                        "Awaiting data".into()
                    }
                }
                other => status_text(other),
            };
            a.set_source_status(text.into());
            a.set_source_status_color(slint::Color::from_rgb_u8(
                if s.last_frame
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                {
                    76
                } else {
                    214
                },
                160,
                90,
            ));
        }
        if s.source.is_some() && s.frame.is_some() {
            let timeout = a.get_face_search_timeout().parse::<u64>().unwrap_or(3000);
            let face = face_present_after_timeout(
                s.frame.as_ref().is_some_and(|f| f.face_present),
                s.last_frame,
                Duration::from_millis(timeout),
            );
            if s.last_preview_face != Some(face) {
                s.preview_dirty = true;
            }
        }
        if let Some(target) = &s.target {
            let status = target.status();
            a.set_target_status_color(match status {
                NetworkStatus::Connected => slint::Color::from_rgb_u8(76, 191, 131),
                NetworkStatus::Error(_) => slint::Color::from_rgb_u8(196, 64, 57),
                _ => slint::Color::from_rgb_u8(214, 162, 60),
            });
            a.set_target_status(status_text(status).into());
        }
        let time_dependent = s
            .preset
            .params
            .iter()
            .any(|p| p.func.contains("Wave") || p.func.contains("PingPong"));
        let advance = s.source.is_some() && s.frame.is_some();
        let should_preview = s.preview_dirty || (advance && (frame_changed || time_dependent));
        if should_preview {
            s.preview_dirty = true;
        }
        drop(s);
        if should_preview {
            self.preview(advance);
        }
    }
}
fn status_text(status: NetworkStatus) -> String {
    match status {
        NetworkStatus::Stopped => "Stopped".into(),
        NetworkStatus::Connecting => "Connecting".into(),
        NetworkStatus::Authorizing => "Authorize in VTube Studio".into(),
        NetworkStatus::Connected => "Connected".into(),
        NetworkStatus::Error(e) => e,
    }
}

pub(crate) fn face_present_after_timeout(
    frame_present: bool,
    last_frame: Option<Instant>,
    timeout: Duration,
) -> bool {
    frame_present && last_frame.is_some_and(|time| time.elapsed() < timeout)
}

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
    macro_rules! bind {($object:expr,$handler:ident,|$u:ident $(,$arg:ident)*| $body:block)=>{{let weak=Rc::downgrade(&ui);$object.$handler(move |$($arg),*|{if let Some($u)=weak.upgrade(){$body}});}};}
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
        }
        slint::CloseRequestResponse::KeepWindowShown
    });
    let timer = Timer::default();
    let weak = Rc::downgrade(&ui);
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(u) = weak.upgrade() {
            u.tick();
        }
    });
    app.run()?;
    timer.stop();
    drop(ui);
    Ok(())
}
