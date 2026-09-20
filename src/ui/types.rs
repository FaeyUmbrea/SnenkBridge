use std::{collections::BTreeMap, time::Instant};

use crate::evaluation::Evaluator;
use crate::model::{Preset, TrackingFrame};
use crate::network::{SourceHandle, TargetHandle};
use crate::presets::{self, PresetStore};
use crate::BLENDSHAPE_NAMES;

#[derive(Clone)]
pub struct Choice {
    pub preset: Preset,
    pub filename: Option<String>,
}

#[derive(Clone)]
pub enum Action {
    Switch(usize),
    New,
    Import,
    Delete,
    Close,
}

pub struct State {
    pub choices: Vec<Choice>,
    pub selected: usize,
    pub preset: Preset,
    pub store: PresetStore,
    pub evaluator: Option<Evaluator>,
    pub source: Option<SourceHandle>,
    pub target: Option<TargetHandle>,
    pub manual: BTreeMap<String, f64>,
    pub frame: Option<TrackingFrame>,
    pub frame_count: u64,
    pub last_frame: Option<Instant>,
    pub rate_at: Instant,
    pub rate_count: u64,
    pub rate: f64,
    pub session: Instant,
    pub pending: Option<Action>,
    pub import_text: Option<(String, bool)>,
    pub stored_selection: bool,
    pub preview_dirty: bool,
    pub last_preview_face: Option<bool>,
    pub last_outputs: Option<BTreeMap<String, f64>>,
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

pub fn builtins() -> Vec<Choice> {
    [
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
            log::error!("Bundled preset {title}: {e}");
            None
        }
    })
    .collect()
}
