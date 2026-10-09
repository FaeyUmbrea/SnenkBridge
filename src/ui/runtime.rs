use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::{
    evaluation::{self, Evaluator},
    model::{Preset, TrackingFrame},
    network::{
        NetworkStatus, SourceHandle, SourceKind, SourceSettings, TargetHandle, TargetSettings,
    },
    presets::{self, PresetStore},
    settings::{self, Settings},
    BLENDSHAPE_NAMES,
};

#[derive(Clone)]
pub struct Choice {
    pub preset: Preset,
    pub filename: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Preview {
    pub inputs: BTreeMap<String, f64>,
    pub outputs: BTreeMap<String, f64>,
    pub source_active: bool,
    pub target_active: bool,
    pub source_status: String,
    pub target_status: String,
    pub face_present: bool,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub preset: Preset,
    pub choices: Vec<String>,
    pub selected: usize,
    pub can_delete: bool,
    pub dirty: bool,
    pub settings: Settings,
    pub errors: Vec<String>,
    pub error: String,
    pub preview: Preview,
    pub variables: Vec<String>,
    pub credits: &'static str,
}

pub struct Runtime {
    pub choices: Vec<Choice>,
    pub selected: usize,
    pub preset: Preset,
    pub stored_selection: bool,
    pub store: PresetStore,
    pub dirty: bool,
    pub settings: Settings,
    pub error: String,
    pub source: Option<SourceHandle>,
    pub target: Option<TargetHandle>,
    pub manual: BTreeMap<String, f64>,
    pub import_text: Option<(String, bool)>,
    evaluator: Option<Evaluator>,
    frame: Option<TrackingFrame>,
    frame_count: u64,
    last_frame: Option<Instant>,
    rate_at: Instant,
    rate_count: u64,
    rate: f64,
    session: Instant,
    face_present: bool,
    inputs: BTreeMap<String, f64>,
    outputs: BTreeMap<String, f64>,
    preview_dirty: bool,
    status_at: Instant,
}

pub fn input_defaults() -> BTreeMap<String, f64> {
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

pub(crate) fn face_present_after_timeout(
    frame_present: bool,
    last_frame: Option<Instant>,
    timeout: Duration,
) -> bool {
    frame_present && last_frame.is_some_and(|time| time.elapsed() < timeout)
}

impl Runtime {
    pub fn new() -> Self {
        Self::load(
            settings::load_settings(),
            PresetStore::new(settings::app_dir().join("presets")),
        )
    }

    pub(super) fn load(settings: Settings, store: PresetStore) -> Self {
        let mut error = String::new();
        let mut choices: Vec<Choice> = [
            ("Default", include_str!("../../presets/default.json")),
            (
                "Maruseu Enhanced",
                include_str!("../../presets/maruseu_enhanced.json"),
            ),
            (
                "Maruseu VBridger",
                include_str!("../../presets/maruseu_vbridger.json"),
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
                log::error!("Bundled preset: {e}");
                None
            }
        })
        .collect();
        match store.list() {
            Ok(stored) => choices.extend(stored.into_iter().map(|c| Choice {
                preset: c.preset,
                filename: Some(c.filename),
            })),
            Err(e) => error = e,
        }
        if choices.is_empty() {
            choices.push(Choice {
                preset: Preset::new("Default", vec![]),
                filename: None,
            });
        }
        let selected = choices
            .iter()
            .position(|c| c.preset.title == settings.preset_name)
            .unwrap_or(0);
        let now = Instant::now();
        let mut runtime = Self {
            preset: choices[selected].preset.clone(),
            stored_selection: choices[selected].filename.is_some(),
            choices,
            selected,
            store,
            settings,
            error: String::new(),
            dirty: false,
            source: None,
            target: None,
            manual: input_defaults(),
            import_text: None,
            evaluator: None,
            frame: None,
            frame_count: 0,
            last_frame: None,
            rate_at: now,
            rate_count: 0,
            rate: 0.0,
            session: now,
            face_present: false,
            inputs: input_defaults(),
            outputs: BTreeMap::new(),
            preview_dirty: true,
            status_at: now,
        };
        runtime.rebuild();
        if !error.is_empty() {
            runtime.error = error;
        }
        runtime
    }

    pub fn ensure_editable(&self) -> Result<(), String> {
        if self.target.is_some() {
            Err("Stop VTube Studio output before editing parameters or presets.".into())
        } else {
            Ok(())
        }
    }

    pub fn persist(&mut self) {
        self.settings.preset_name = self.preset.title.clone();
        settings::save_settings(&self.settings);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            preset: self.preset.clone(),
            choices: self
                .choices
                .iter()
                .map(|c| c.preset.title.clone())
                .collect(),
            selected: self.selected,
            can_delete: self.stored_selection,
            dirty: self.dirty,
            settings: self.settings.clone(),
            errors: evaluation::validation_errors(&self.preset.params),
            error: self.error.clone(),
            preview: self.preview(),
            variables: input_defaults().into_keys().collect(),
            credits: crate::DEPENDENCY_CREDITS,
        }
    }

    pub fn rebuild(&mut self) {
        match Evaluator::new(&self.preset.params) {
            Ok(e) => {
                self.evaluator = Some(e);
                self.error.clear();
            }
            Err(e) => {
                self.evaluator = None;
                self.error = e;
            }
        }
        self.evaluate(false);
    }

    pub fn add(&mut self, preset: Preset) {
        self.preset = preset.clone();
        self.choices.push(Choice {
            preset,
            filename: None,
        });
        self.selected = self.choices.len() - 1;
        self.stored_selection = false;
        self.dirty = true;
        self.rebuild();
    }

    pub fn save(&mut self) -> Result<(), String> {
        if self.preset.title.trim().is_empty() {
            return Err("A preset needs a title.".into());
        }
        let filename = self.store.save(&self.preset)?;
        let choice = Choice {
            preset: self.preset.clone(),
            filename: Some(filename.clone()),
        };
        if !self.stored_selection {
            self.choices[self.selected] = choice;
        } else if let Some(index) = self
            .choices
            .iter()
            .position(|c| c.filename.as_ref() == Some(&filename))
        {
            self.choices[index] = choice;
            self.selected = index;
        } else {
            self.choices.push(choice);
            self.selected = self.choices.len() - 1;
        }
        self.stored_selection = true;
        self.dirty = false;
        self.error.clear();
        self.persist();
        Ok(())
    }

    pub fn toggle_source(&mut self) -> Result<(), String> {
        if self.source.take().is_some() {
            if let Some(target) = &self.target {
                target.publish(self.outputs.clone(), false);
            }
            self.frame = None;
            self.last_frame = None;
            self.evaluate(false);
        } else {
            self.settings
                .face_search_timeout
                .parse::<u64>()
                .map_err(|_| "Face-loss timeout must be a nonnegative number of milliseconds.")?;
            self.frame = None;
            self.last_frame = None;
            self.frame_count = 0;
            self.rate_count = 0;
            self.rate_at = Instant::now();
            self.rate = 0.0;
            self.source = Some(SourceHandle::start(SourceSettings {
                kind: if self.settings.tracking_type_index == 1 {
                    SourceKind::IFacialMocap
                } else {
                    SourceKind::VTubeStudio
                },
                phone_address: self.settings.phone_ip.clone(),
            }));
        }
        self.preview_dirty = true;
        self.persist();
        Ok(())
    }

    pub fn toggle_target(&mut self) -> Result<(), String> {
        if self.target.take().is_none() {
            let port = self
                .settings
                .vts_port
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or("VTube Studio port must be between 1 and 65535.")?;
            if self.evaluator.is_none() {
                return Err("Fix preset validation errors before starting output.".into());
            }
            self.target = Some(TargetHandle::start(
                TargetSettings {
                    host: self.settings.vts_ip.clone(),
                    port,
                    token_path: settings::app_dir().join("vts-token.json"),
                },
                self.preset.params.clone(),
            ));
        }
        self.preview_dirty = true;
        self.persist();
        Ok(())
    }

    pub fn evaluate(&mut self, advance: bool) {
        let live = self.source.is_some();
        let mut values = if live {
            self.frame
                .as_ref()
                .map(|f| f.values.clone())
                .unwrap_or_else(input_defaults)
        } else {
            self.manual.clone()
        };
        let face = if live {
            face_present_after_timeout(
                self.frame.as_ref().is_some_and(|f| f.face_present),
                self.last_frame,
                Duration::from_millis(self.settings.face_search_timeout.parse().unwrap_or(3000)),
            )
        } else {
            values.get("FaceFound").copied().unwrap_or(0.0) > 0.0
        };
        values.insert("FaceFound".into(), f64::from(face));
        values.extend(evaluation::time_variables(
            &self.preset.params,
            self.session.elapsed().as_millis() as u64,
        ));
        self.outputs = self
            .evaluator
            .as_mut()
            .map(|e| e.evaluate(&values, advance))
            .unwrap_or_default();
        if live && self.frame.is_some() {
            if let Some(target) = &self.target {
                target.publish(self.outputs.clone(), face);
            }
        }
        self.inputs = values;
        self.face_present = face;
        self.preview_dirty = true;
    }

    fn preview(&self) -> Preview {
        let source_status = self
            .source
            .as_ref()
            .map(|source| match source.snapshot().0 {
                NetworkStatus::Connected
                    if self
                        .last_frame
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(1)) =>
                {
                    format!("Receiving · {:.0} fps", self.rate)
                }
                NetworkStatus::Connected if self.frame.is_some() => "Interrupted".into(),
                NetworkStatus::Connected => "Awaiting data".into(),
                status => status_text(status),
            })
            .unwrap_or_else(|| "Stopped".into());
        Preview {
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            source_active: self.source.is_some(),
            target_active: self.target.is_some(),
            source_status,
            target_status: self
                .target
                .as_ref()
                .map(|t| status_text(t.status()))
                .unwrap_or_else(|| "Stopped".into()),
            face_present: self.face_present,
        }
    }

    pub fn tick(&mut self) -> Option<Preview> {
        let mut changed = false;
        if let Some(source) = &self.source {
            let (_, frame, count) = source.snapshot();
            if count != self.frame_count {
                self.frame_count = count;
                self.frame = frame;
                self.last_frame = Some(Instant::now());
                changed = true;
            }
            let elapsed = self.rate_at.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                self.rate = count.saturating_sub(self.rate_count) as f64 / elapsed;
                self.rate_count = count;
                self.rate_at = Instant::now();
            }
        }
        let advance = self.source.is_some() && self.frame.is_some();
        let face = advance
            && face_present_after_timeout(
                self.frame.as_ref().is_some_and(|f| f.face_present),
                self.last_frame,
                Duration::from_millis(self.settings.face_search_timeout.parse().unwrap_or(3000)),
            );
        let time_dependent = self
            .preset
            .params
            .iter()
            .any(|p| p.func.contains("Wave") || p.func.contains("PingPong"));
        if advance && (changed || time_dependent || face != self.face_present) {
            self.evaluate(true);
        }
        let status_due = (self.source.is_some() || self.target.is_some())
            && self.status_at.elapsed() >= Duration::from_millis(250);
        if self.preview_dirty || status_due {
            self.preview_dirty = false;
            self.status_at = Instant::now();
            Some(self.preview())
        } else {
            None
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
