use crate::{
    evaluation::{time_variables, validation_errors, Evaluator},
    model::{DelaySettings, Parameter, Preset},
    presets::{convert_expression, import_vitamins, parse, PresetStore},
    ui::face_present_after_timeout,
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn parameter(name: &str, func: &str) -> Parameter {
    Parameter {
        name: name.into(),
        func: func.into(),
        min: -100.0,
        max: 100.0,
        default_value: 7.0,
        delay_buffer: None,
    }
}
fn delayed(name: &str, reference: &str, count: usize, smoothing: f64) -> Parameter {
    let mut p = parameter(name, "");
    p.delay_buffer = Some(DelaySettings {
        ref_param: reference.into(),
        delay_count: count,
        smoothing,
        in_min: 0.0,
        in_max: 10.0,
        out_min: 0.0,
        out_max: 100.0,
    });
    p
}

#[test]
fn math_conversion_evaluates_all_supported_functions() {
    for (source, expected) in [
        ("Math.abs(-2)", 2.0),
        ("Math.min(2,3)", 2.0),
        ("Math.max(2,3)", 3.0),
        ("Math.sin(0)", 0.0),
        ("Math.cos(0)", 1.0),
        ("Math.floor(2.5)", 2.0),
        ("Math.ceil(2.5)", 3.0),
        ("Math.sqrt(4)", 2.0),
        ("Math.pow(2,3)", 8.0),
        ("Math.PI", std::f64::consts::PI),
    ] {
        let mut evaluator =
            Evaluator::new(&[parameter("value", &convert_expression(source))]).unwrap();
        assert!(
            (evaluator.evaluate(&BTreeMap::new(), true)["value"] - expected).abs() < 1e-9,
            "{source}"
        );
    }
}

#[test]
fn delay_order_state_and_fallback() {
    let mut evaluator = Evaluator::new(&[
        delayed("second", "first", 1, 1.0),
        delayed("first", "raw", 3, 2.0),
        parameter("raw", "Input"),
    ])
    .unwrap();
    let inputs = BTreeMap::from([(String::from("Input"), 10.0)]);
    assert_eq!(evaluator.evaluate(&inputs, true)["first"], 0.0);
    assert_eq!(evaluator.evaluate(&inputs, false)["first"], 0.0);
    assert_eq!(evaluator.evaluate(&inputs, true)["first"], 0.0);
    assert_eq!(evaluator.evaluate(&inputs, true)["first"], 50.0);
    assert_eq!(evaluator.evaluate(&inputs, true)["second"], 100.0);
    assert_eq!(evaluator.evaluate(&BTreeMap::new(), false)["raw"], 7.0);
}

#[test]
fn validation_is_per_row_and_runtime_values_are_finite() {
    assert!(Evaluator::new(&[delayed("a", "b", 1, 1.0), delayed("b", "a", 1, 1.0)]).is_err());
    let errors = validation_errors(&[
        parameter("a", ""),
        parameter("b", "1+2"),
        parameter("c", "("),
    ]);
    assert_eq!(errors.len(), 3);
    assert!(!errors[0].is_empty() && errors[1].is_empty() && !errors[2].is_empty());
    let mut evaluator =
        Evaluator::new(&[parameter("huge", "2000000"), parameter("bad", "0.0/0.0")]).unwrap();
    let output = evaluator.evaluate(&BTreeMap::new(), true);
    assert_eq!(output["huge"], 1_000_000.0);
    assert_eq!(output["bad"], 7.0);
}

#[test]
fn identifiers_division_and_time_variables() {
    assert_eq!(
        convert_expression(" return jawOpen / 2; // hi"),
        "JawOpen / 2"
    );
    assert_eq!(
        convert_expression("eyeBlink_L + jawOpen_extra + obj.jawOpen + headRotY + mystery"),
        "EyeBlinkLeft + jawOpen_extra + obj.jawOpen + HeadRotY + mystery"
    );
    let values = time_variables(&[parameter("wave", "Wave1000 + PingPong1000 + Wave0")], 250);
    assert_eq!(values["Wave1000"], 0.5);
    assert_eq!(values["PingPong1000"], 0.25);
    assert!(!values.contains_key("Wave0"));
}

#[test]
fn store_roundtrip_collision_and_traversal_checks() {
    let directory = tempfile::tempdir().unwrap();
    let store = PresetStore::new(directory.path().into());
    let mut first = Preset::new("Hello!", vec![parameter("a", "2")]);
    first.author = "Faey".into();
    assert_eq!(store.save(&first).unwrap(), "hello.snek");
    first.description = "changed".into();
    assert_eq!(store.save(&first).unwrap(), "hello.snek");
    assert_eq!(
        store.save(&Preset::new("Hello?", vec![])).unwrap(),
        "hello-2.snek"
    );
    assert_eq!(store.load("hello.snek").unwrap(), first);
    assert!(store.delete("../escape.snek").is_err());
    assert!(store.load("/tmp/a.snek").is_err());
    assert!(parse(r#"{"format":"other","version":1,"title":"","params":[]}"#).is_err());
    assert!(parse(r#"{"format":"snek","version":2,"title":"","params":[]}"#).is_err());
    assert_eq!(parse("[]").unwrap().title, "");
}

#[test]
fn vitamins_metadata_delay_curve_and_realistic_declarations() {
    let text = r#"{"saveName":"Test","author":"Author","description":"Info","customParam":[{"sendFlag":"false"},{"sendFlag":"true","paramName":"param_FaceAngleX","func":"return headRotX / 2;","min":-10,"max":10,"default":1},{"sendFlag":"true","paramName":"Eye_Squint_L","func":"var result = jawOpen; var outmin = 0, outmax = 20; curve(result);","min":0,"max":10,"default":1},{"sendFlag":"true","paramName":"delay","func":"let p=ref.FaceAngleX; let s=2, dC=3, inmin=-10, inmax=10, outmin=0, outmax=20;","min":0,"max":10,"default":1}]}"#;
    let result = import_vitamins(text, true).unwrap();
    assert_eq!(result.preset.params.len(), 3);
    assert_eq!(result.preset.params[0].name, "FaceAngleY");
    assert_eq!(result.preset.params[1].max, 20.0);
    let delay = result.preset.params[2].delay_buffer.as_ref().unwrap();
    assert_eq!(delay.ref_param, "FaceAngleX");
    assert_eq!(delay.delay_count, 3);
    assert_eq!(delay.out_max, 20.0);
    assert_eq!(result.warnings.len(), 2);
}

#[test]
fn face_loss_timeout_changes_state_without_new_packets() {
    let recent = Instant::now();
    assert!(face_present_after_timeout(
        true,
        Some(recent),
        Duration::from_secs(3)
    ));
    assert!(!face_present_after_timeout(
        true,
        Some(recent - Duration::from_secs(4)),
        Duration::from_secs(3)
    ));
    assert!(!face_present_after_timeout(
        false,
        Some(recent),
        Duration::from_secs(3)
    ));
}
