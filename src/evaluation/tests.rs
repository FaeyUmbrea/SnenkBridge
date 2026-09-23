use super::*;

fn parameter(expression: &str) -> Parameter {
    Parameter {
        name: "result".into(),
        func: expression.into(),
        min: -100.0,
        max: 100.0,
        default_value: 7.0,
        delay_buffer: None,
    }
}

#[test]
fn existing_expression_language_is_preserved() {
    // These results were checked against evalexpr 13.1. Keep integer arithmetic,
    // tuple arguments, conditional typing and chained expressions compatible.
    for (expression, expected) in [
        ("5 / 2", 2.0),
        ("5 / 2.0", 2.5),
        ("-5 / 2", -2.0),
        ("-5 % 2", -1.0),
        ("2 + 3 * 4", 14.0),
        ("2 ^ 3 ^ 2", 64.0),
        ("-2 ^ 2", -4.0),
        ("math::pow(2, 3)", 8.0),
        ("math::min(4, -2, 3)", -2.0),
        ("math::max(4, -2, 3)", 4.0),
        ("if(Input > 0 && Input < 1, math::abs(-3), 8)", 3.0),
        ("if(1 == 1.0, 3, 8)", 8.0),
        ("len(\"hello\")", 5.0),
        ("len((1, 2, 3))", 3.0),
        ("len(str::substring(\"hello\", 1, 3))", 2.0),
        ("1; 2; 3", 3.0),
        ("0x10 + 0b10 + 0o10", 26.0),
        ("-0b11 / 0o10", 0.0),
        ("0b1/* a comment */0 + 0o1// another comment\n0", 10.0),
        ("len(\"0b10 // 0o10\")", 12.0),
        ("if(\"0b10\" == \"2\", 1, 0)", 0.0),
        ("if(0b111111111111111111111111111111111111111111111111111111111111111 == 9223372036854775807, 1, 0)", 1.0),
        ("if(0o777777777777777777777 == 9223372036854775807, 1, 0)", 1.0),
        ("1e-3 * 1000", 1.0),
        ("Input / 2", 0.25),
    ] {
        let params = [parameter(expression)];
        assert_eq!(validation_errors(&params), [""], "{expression}");
        let values = BTreeMap::from([("Input".into(), 0.5)]);
        let result = Evaluator::new(&params).unwrap().evaluate(&values, true);
        assert_eq!(result["result"], expected, "{expression}");
    }
}

#[test]
fn invalid_and_non_numeric_results_keep_their_fallback_behavior() {
    for expression in [
        "Missing",
        "1 / 0",
        "0.0 / 0.0",
        "math::sqrt(-1)",
        "9223372036854775807 + 1",
        "true",
        "\"hello\"",
        "(1, 2)",
        "unknown(1)",
        "math::pow(2)",
        "x = 3; x",
        "if(true, 3, Missing)",
        "1 +",
        "1e-0b1",
        "1e+0o1",
    ] {
        let result = Evaluator::new(&[parameter(expression)])
            .unwrap()
            .evaluate(&BTreeMap::new(), true);
        assert_eq!(result["result"], 7.0, "{expression}");
    }
    for expression in ["", "(", "/* unclosed", "\"unclosed", "0b10(2)", "0o10 2"] {
        assert!(
            !validation_errors(&[parameter(expression)])[0].is_empty(),
            "{expression}"
        );
        assert!(
            Evaluator::new(&[parameter(expression)])
                .unwrap()
                .evaluate(&BTreeMap::new(), true)
                .is_empty(),
            "{expression}"
        );
    }
}

#[test]
fn literals_preserve_identifiers_strings_and_time_variables() {
    let expression = "Wave1000 * 0b10 + PingPong2000 / 0o10 + My0b10 + 0b102";
    let params = [parameter(expression)];
    let mut values = time_variables(&params, 250);
    assert_eq!(
        values,
        BTreeMap::from([("Wave1000".into(), 0.5), ("PingPong2000".into(), 0.125)])
    );
    values.insert("My0b10".into(), 3.0);
    values.insert("0b102".into(), 4.0);
    assert_eq!(
        Evaluator::new(&params).unwrap().evaluate(&values, true)["result"],
        8.015625
    );

    for identifier in [
        "0b",
        "0o",
        "0B10",
        "0O10",
        "0b12",
        "0o8",
        "0b10suffix",
        "0b10.0",
        "0b1000000000000000000000000000000000000000000000000000000000000000",
        "0o1000000000000000000000",
    ] {
        let params = [parameter(identifier)];
        let values = BTreeMap::from([(identifier.into(), 11.0)]);
        assert_eq!(
            Evaluator::new(&params).unwrap().evaluate(&values, true)["result"],
            11.0,
            "{identifier}"
        );
    }
    assert_eq!(
        parse_expression(r#"len("a\"0b10\\0o10") + 0b10"#)
            .unwrap()
            .evaluate(&BTreeMap::new())
            .unwrap(),
        13.0
    );
}

#[test]
fn shipped_presets_match_recorded_outputs() {
    // Recorded with evalexpr 13.1.0 before changing the evaluator dependency.
    // The deterministic sequence exercises every tracking channel and retains
    // evaluator state across frames so delay buffers are covered as well.
    let expected: BTreeMap<String, Vec<BTreeMap<String, f64>>> =
        serde_json::from_str(include_str!("preset_outputs.json")).unwrap();
    for (name, source) in [
        (
            "presets/default.json",
            include_str!("../../presets/default.json"),
        ),
        (
            "presets/maruseu_vbridger.json",
            include_str!("../../presets/maruseu_vbridger.json"),
        ),
        (
            "presets/maruseu_enhanced.json",
            include_str!("../../presets/maruseu_enhanced.json"),
        ),
        (
            "examples/test.json",
            include_str!("../../examples/test.json"),
        ),
    ] {
        let params: Vec<Parameter> = serde_json::from_str(source).unwrap();
        assert!(
            validation_errors(&params).iter().all(String::is_empty),
            "{name}"
        );
        let mut evaluator = Evaluator::new(&params).unwrap();
        for (step, expected) in expected[name].iter().enumerate() {
            let mut inputs: BTreeMap<String, f64> = include_str!("../../resources/tracking_data")
                .lines()
                .filter(|name| !name.is_empty())
                .enumerate()
                .map(|(i, name)| (name.to_owned(), ((i * 17 + step * 23) % 101) as f64 / 100.0))
                .collect();
            inputs.extend(time_variables(&params, step as u64 * 137));
            let output = evaluator.evaluate(&inputs, true);
            assert_eq!(
                output.keys().collect::<Vec<_>>(),
                expected.keys().collect::<Vec<_>>(),
                "{name}, frame {step}"
            );
            for (parameter, expected) in expected {
                assert!(
                    (output[parameter] - expected).abs() <= 1e-12 * expected.abs().max(1.0),
                    "{name}, frame {step}, {parameter}: {} != {expected}",
                    output[parameter]
                );
            }
        }
    }
}
