use super::runtime::Runtime;
use crate::{
    model::{DelaySettings, Parameter, Preset},
    presets::PresetStore,
    settings::Settings,
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn runtime() -> (tempfile::TempDir, Runtime) {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::load(Settings::default(), PresetStore::new(dir.path().into()));
    (dir, runtime)
}

#[test]
fn manual_edits_do_not_advance_delay_buffers() {
    let (_dir, mut runtime) = runtime();
    let param = Parameter {
        name: "Delayed".into(),
        func: String::new(),
        min: 0.0,
        max: 1.0,
        default_value: 0.0,
        delay_buffer: Some(DelaySettings {
            ref_param: "JawOpen".into(),
            smoothing: 1.0,
            delay_count: 2,
            in_min: 0.0,
            in_max: 1.0,
            out_min: 0.0,
            out_max: 1.0,
        }),
    };
    runtime.add(Preset::new("Delay", vec![param]));
    runtime.manual.insert("JawOpen".into(), 1.0);
    runtime.evaluate(false);
    assert_eq!(runtime.snapshot().preview.outputs["Delayed"], 0.0);
    runtime.evaluate(true);
    assert_eq!(runtime.snapshot().preview.outputs["Delayed"], 0.0);
    runtime.evaluate(true);
    assert_eq!(runtime.snapshot().preview.outputs["Delayed"], 1.0);
}

#[test]
fn invalid_edits_remain_visible_and_disable_evaluation() {
    let (_dir, mut runtime) = runtime();
    let mut preset = Preset::new("Invalid", vec![]);
    preset.params.push(Parameter {
        name: "Bad".into(),
        func: "(".into(),
        min: 0.0,
        max: 1.0,
        default_value: 0.0,
        delay_buffer: None,
    });
    runtime.add(preset);
    let snapshot = runtime.snapshot();
    assert!(snapshot.dirty);
    assert!(!snapshot.errors[0].is_empty());
    assert_eq!(snapshot.preview.outputs, BTreeMap::new());
}

#[test]
fn offline_tick_is_idle_after_initial_snapshot() {
    let (_dir, mut runtime) = runtime();
    assert!(runtime.tick().is_some());
    assert!(runtime.tick().is_none());
    runtime.manual.insert("JawOpen".into(), 0.7);
    runtime.evaluate(false);
    assert_eq!(runtime.tick().unwrap().inputs["JawOpen"], 0.7);
    assert!(runtime.tick().is_none());
}

#[test]
fn expired_face_is_not_present() {
    assert!(!super::face_present_after_timeout(
        true,
        Some(Instant::now() - Duration::from_secs(4)),
        Duration::from_secs(3)
    ));
}

#[test]
fn bundled_preset_names_and_saved_selection_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        preset_name: "Maruseu Enhanced".into(),
        ..Settings::default()
    };
    let runtime = Runtime::load(settings, PresetStore::new(dir.path().into()));
    let snapshot = runtime.snapshot();
    assert_eq!(
        snapshot.choices,
        ["Default", "Maruseu Enhanced", "Maruseu VBridger"]
    );
    assert_eq!(snapshot.selected, 1);
    assert!(!snapshot.can_delete);
}
