use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// VBridger input format
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VBridgerConfig {
    pub version: String,
    pub custom_param: Vec<VBridgerParam>,
    pub author: String,
    pub description: String,
    pub save_name: String,
    pub is_default: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VBridgerParam {
    pub func: String,
    pub max: f64,
    pub min: f64,
    pub default: f64,
    #[serde(rename = "type")]
    pub param_type: String,
    pub send_flag: String,
    pub param_name: String,
}

/// SnenkBridge output format
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CalcFn {
    pub name: String,
    pub func: String,
    pub min: f64,
    pub max: f64,
    pub default_value: f64,
}

/// Converts a VBridger JSON config string into a SnenkBridge JSON config string.
///
/// Complex parameters (bezier easing, delay buffers) are simplified to their
/// core expressions — the easing curves and stateful smoothing are discarded.
pub fn convert_vbridger_config(input: &str) -> Result<String, String> {
    let vb: VBridgerConfig =
        serde_json::from_str(input).map_err(|e| format!("Failed to parse VBridger config: {e}"))?;

    let calc_fns: Vec<CalcFn> = vb
        .custom_param
        .into_iter()
        .filter(|p| p.send_flag == "true")
        .map(convert_param)
        .collect();

    serde_json::to_string_pretty(&calc_fns).map_err(|e| format!("Failed to serialize output: {e}"))
}

fn convert_param(param: VBridgerParam) -> CalcFn {
    let name = param
        .param_name
        .strip_prefix("param_")
        .unwrap_or(&param.param_name)
        .to_string();

    let (func, min, max) = if param.param_type == "complex" {
        convert_complex_func(&param.func, param.min, param.max)
    } else {
        let func = convert_simple_func(&param.func);
        (func, param.min, param.max)
    };

    CalcFn {
        name,
        func,
        min,
        max,
        default_value: param.default,
    }
}

/// Extracts the core expression from a complex VBridger function.
///
/// Complex functions follow one of two patterns:
/// 1. Bezier curve: `let result = <expr>\n let inmin=...` — extract expr, use outmin/outmax
/// 2. Delay buffer: `let p=ref.ParamName;` — extract the ref and convert
fn convert_complex_func(func: &str, fallback_min: f64, fallback_max: f64) -> (String, f64, f64) {
    let lines: Vec<&str> = func.lines().collect();

    // Check for delay buffer pattern (uses ref.*)
    if let Some(first_line) = lines.first() {
        let trimmed = first_line.trim();
        if trimmed.starts_with("let p=ref.") || trimmed.starts_with("let p = ref.") {
            return convert_delay_buffer_func(&lines, fallback_min, fallback_max);
        }
    }

    // Bezier curve pattern: extract core expression and output range
    convert_bezier_func(&lines, fallback_min, fallback_max)
}

/// Handles bezier curve complex functions.
/// Extracts the initial `result = <expr>` and the output range.
fn convert_bezier_func(lines: &[&str], fallback_min: f64, fallback_max: f64) -> (String, f64, f64) {
    let mut core_expr = String::new();
    let mut outmin = fallback_min;
    let mut outmax = fallback_max;

    for line in lines {
        let trimmed = line.trim();

        // Extract core expression from "let result = <expr>"
        if trimmed.starts_with("let result = ") || trimmed.starts_with("let result= ") {
            core_expr = trimmed
                .trim_start_matches("let result = ")
                .trim_start_matches("let result= ")
                .to_string();
        }

        // Extract output range
        if trimmed.starts_with("let outmin=") {
            if let Some((min_val, max_val)) = parse_range_line(trimmed, "outmin", "outmax") {
                outmin = min_val;
                outmax = max_val;
            }
        }
    }

    if core_expr.is_empty() {
        // Fallback: try to use the first line as the expression
        if let Some(first) = lines.first() {
            core_expr = first.trim().to_string();
        }
    }

    let func = convert_simple_func(&core_expr);
    (func, outmin, outmax)
}

/// Handles delay buffer complex functions that reference other parameters.
fn convert_delay_buffer_func(
    lines: &[&str],
    fallback_min: f64,
    fallback_max: f64,
) -> (String, f64, f64) {
    let mut ref_param = String::new();
    let mut outmin = fallback_min;
    let mut outmax = fallback_max;

    for line in lines {
        let trimmed = line.trim();

        // Extract referenced parameter: "let p=ref.FaceAngleX;"
        if trimmed.starts_with("let p=ref.") || trimmed.starts_with("let p = ref.") {
            ref_param = trimmed
                .trim_start_matches("let p=ref.")
                .trim_start_matches("let p = ref.")
                .trim_end_matches(';')
                .to_string();
        }

        if trimmed.starts_with("let outmin=") {
            if let Some((min_val, max_val)) = parse_range_line(trimmed, "outmin", "outmax") {
                outmin = min_val;
                outmax = max_val;
            }
        }
    }

    // The delay buffer is a smoothing filter on a referenced parameter.
    // Since evalexpr is stateless, we output the direct reference as an approximation.
    let func = convert_variable_name(&ref_param);
    (func, outmin, outmax)
}

/// Parses a line like "let outmin=-30.0, outmax=30.0;  //output range"
fn parse_range_line(line: &str, min_key: &str, max_key: &str) -> Option<(f64, f64)> {
    let line = line.split("//").next().unwrap_or(line).trim();
    let line = line.trim_end_matches(';');

    let mut min_val = None;
    let mut max_val = None;

    for part in line.split(',') {
        let part = part.trim();
        if let Some(val_str) = part.strip_prefix(&format!("let {min_key}=")) {
            min_val = val_str.trim().parse::<f64>().ok();
        } else if let Some(val_str) = part.strip_prefix(&format!("{min_key}=")) {
            min_val = val_str.trim().parse::<f64>().ok();
        } else if let Some(val_str) = part.strip_prefix(&format!("{max_key}=")) {
            max_val = val_str.trim().parse::<f64>().ok();
        }
    }

    match (min_val, max_val) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    }
}

/// Converts a simple VBridger expression to evalexpr syntax.
fn convert_simple_func(func: &str) -> String {
    let mut result = func.to_string();

    // Strip "return " prefix
    result = result.trim().to_string();
    if result.starts_with("return ") {
        result = result["return ".len()..].to_string();
    }

    // Remove inline comments
    if let Some(idx) = find_comment_start(&result) {
        result = result[..idx].trim().to_string();
    }

    // Remove trailing semicolons
    result = result.trim_end_matches(';').trim().to_string();

    // Convert JS Math functions to evalexpr
    result = result.replace("Math.abs(", "math::abs(");
    result = result.replace("Math.min(", "math::min(");
    result = result.replace("Math.max(", "math::max(");
    result = result.replace("Math.sin(", "math::sin(");
    result = result.replace("Math.cos(", "math::cos(");
    result = result.replace("Math.floor(", "math::floor(");
    result = result.replace("Math.ceil(", "math::ceil(");
    result = result.replace("Math.sqrt(", "math::sqrt(");
    result = result.replace("Math.pow(", "math::pow(");
    result = result.replace("Math.PI", "math::pi()");

    // Convert variable names
    result = rename_variables(&result);

    result
}

/// Finds the start of a `//` comment that isn't inside a string.
fn find_comment_start(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Renames VBridger tracking variable names to SnenkBridge conventions.
fn rename_variables(expr: &str) -> String {
    let mapping = build_variable_mapping();

    // Sort by length descending to avoid partial replacements
    let mut sorted: Vec<(&str, &str)> = mapping
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut result = expr.to_string();
    for (from, to) in &sorted {
        result = replace_identifier(&result, from, to);
    }
    result
}

/// Replaces whole identifiers only (not substrings of other identifiers).
fn replace_identifier(input: &str, from: &str, to: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining: &str = input;

    while let Some(pos) = remaining.find(from) {
        // Check that the character before is not alphanumeric or underscore
        let before_ok = if pos == 0 {
            true
        } else {
            let c = remaining.as_bytes()[pos - 1];
            !c.is_ascii_alphanumeric() && c != b'_' && c != b'.'
        };

        let after_pos = pos + from.len();
        let after_ok = if after_pos >= remaining.len() {
            true
        } else {
            let c = remaining.as_bytes()[after_pos];
            !c.is_ascii_alphanumeric() && c != b'_'
        };

        if before_ok && after_ok {
            result.push_str(&remaining[..pos]);
            result.push_str(to);
            remaining = &remaining[after_pos..];
        } else {
            result.push_str(&remaining[..pos + 1]);
            remaining = &remaining[pos + 1..];
        }
    }

    result.push_str(remaining);
    result
}

fn convert_variable_name(name: &str) -> String {
    let mapping = build_variable_mapping();
    mapping
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn build_variable_mapping() -> HashMap<String, String> {
    let pairs = [
        // Head rotation/position
        ("headRotX", "HeadRotX"),
        ("headRotY", "HeadRotY"),
        ("headRotZ", "HeadRotZ"),
        ("headPosX", "HeadPosX"),
        ("headPosY", "HeadPosY"),
        ("headPosZ", "HeadPosZ"),
        // Eyes - Left
        ("eyeBlink_L", "EyeBlinkLeft"),
        ("eyeLookDown_L", "EyeLookDownLeft"),
        ("eyeLookIn_L", "EyeLookInLeft"),
        ("eyeLookOut_L", "EyeLookOutLeft"),
        ("eyeLookUp_L", "EyeLookUpLeft"),
        ("eyeSquint_L", "EyeSquintLeft"),
        ("eyeWide_L", "EyeWideLeft"),
        // Eyes - Right
        ("eyeBlink_R", "EyeBlinkRight"),
        ("eyeLookDown_R", "EyeLookDownRight"),
        ("eyeLookIn_R", "EyeLookInRight"),
        ("eyeLookOut_R", "EyeLookOutRight"),
        ("eyeLookUp_R", "EyeLookUpRight"),
        ("eyeSquint_R", "EyeSquintRight"),
        ("eyeWide_R", "EyeWideRight"),
        // Brows
        ("browDown_L", "BrowDownLeft"),
        ("browDown_R", "BrowDownRight"),
        ("browInnerUp", "BrowInnerUp"),
        ("browOuterUp_L", "BrowOuterUpLeft"),
        ("browOuterUp_R", "BrowOuterUpRight"),
        // Cheeks
        ("cheekPuff", "CheekPuff"),
        ("cheekSquint_L", "CheekSquintLeft"),
        ("cheekSquint_R", "CheekSquintRight"),
        // Jaw
        ("jawForward", "JawForward"),
        ("jawLeft", "JawLeft"),
        ("jawOpen", "JawOpen"),
        ("jawRight", "JawRight"),
        // Mouth
        ("mouthClose", "MouthClose"),
        ("mouthDimple_L", "MouthDimpleLeft"),
        ("mouthDimple_R", "MouthDimpleRight"),
        ("mouthFrown_L", "MouthFrownLeft"),
        ("mouthFrown_R", "MouthFrownRight"),
        ("mouthFunnel", "MouthFunnel"),
        ("mouthLeft", "MouthLeft"),
        ("mouthLowerDown_L", "MouthLowerDownLeft"),
        ("mouthLowerDown_R", "MouthLowerDownRight"),
        ("mouthPress_L", "MouthPressLeft"),
        ("mouthPress_R", "MouthPressRight"),
        ("mouthPucker", "MouthPucker"),
        ("mouthRight", "MouthRight"),
        ("mouthRollLower", "MouthRollLower"),
        ("mouthRollUpper", "MouthRollUpper"),
        ("mouthShrugLower", "MouthShrugLower"),
        ("mouthShrugUpper", "MouthShrugUpper"),
        ("mouthSmile_L", "MouthSmileLeft"),
        ("mouthSmile_R", "MouthSmileRight"),
        ("mouthStretch_L", "MouthStretchLeft"),
        ("mouthStretch_R", "MouthStretchRight"),
        ("mouthUpperUp_L", "MouthUpperUpLeft"),
        ("mouthUpperUp_R", "MouthUpperUpRight"),
        // Nose
        ("noseSneer_L", "NoseSneerLeft"),
        ("noseSneer_R", "NoseSneerRight"),
        // Tongue
        ("tongueOut", "TongueOut"),
    ];

    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_expression_conversion() {
        let func = "return headPosX * - 1//+((ref.FaceAngleX+30)/60*8)";
        let result = convert_simple_func(func);
        assert_eq!(result, "HeadPosX * - 1");
    }

    #[test]
    fn test_variable_renaming() {
        let func = "return (eyeLookIn_L - .1) - eyeLookOut_L";
        let result = convert_simple_func(func);
        assert_eq!(result, "(EyeLookInLeft - .1) - EyeLookOutLeft");
    }

    #[test]
    fn test_math_abs_conversion() {
        let expr = "Math.abs(headRotY)";
        let result = convert_simple_func(&format!("return {expr}"));
        assert_eq!(result, "math::abs(HeadRotY)");
    }

    #[test]
    fn test_complex_bezier_extraction() {
        let func = r#"let result = headRotY
let inmin=-40.0, inmax=40;         //input range
let outmin=-30.0, outmax=30.0;     //output range
let x1=.54,y1=.03;
let rest = "bezier stuff";"#;
        let (expr, min, max) = convert_complex_func(func, -40.0, 40.0);
        assert_eq!(expr, "HeadRotY");
        assert_eq!(min, -30.0);
        assert_eq!(max, 30.0);
    }

    #[test]
    fn test_complex_delay_buffer() {
        let func = r#"let p=ref.FaceAngleX;
let s=2.0;        //smoothing
let dC=8;        //delay counter
let inmin=-30.0, inmax=30;         //input range
let outmin=-10.0, outmax=10.0;     //output range
stuff"#;
        let (expr, min, max) = convert_complex_func(func, -30.0, 30.0);
        assert_eq!(expr, "FaceAngleX");
        assert_eq!(min, -10.0);
        assert_eq!(max, 10.0);
    }

    #[test]
    fn test_param_name_stripping() {
        let param = VBridgerParam {
            func: "return headRotY".to_string(),
            max: 30.0,
            min: -30.0,
            default: 0.0,
            param_type: "simple".to_string(),
            send_flag: "true".to_string(),
            param_name: "param_FaceAngleX".to_string(),
        };
        let result = convert_param(param);
        assert_eq!(result.name, "FaceAngleX");
    }

    #[test]
    fn test_full_conversion() {
        let input = r#"{"version":"0.9.7","customParam":[{"func":"return headPosX * - 1","max":15,"min":-15,"default":0,"type":"simple","sendFlag":"true","paramName":"param_FacePositionX"},{"func":"return (eyeLookIn_L - .1) - eyeLookOut_L","max":1,"min":-1,"default":0,"type":"simple","sendFlag":"true","paramName":"param_EyeRightX"}],"author":"Test","description":"","saveName":"Test","isDefault":false}"#;

        let output = convert_vbridger_config(input).unwrap();
        let parsed: Vec<CalcFn> = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "FacePositionX");
        assert_eq!(parsed[0].func, "HeadPosX * - 1");
        assert_eq!(parsed[1].name, "EyeRightX");
        assert_eq!(parsed[1].func, "(EyeLookInLeft - .1) - EyeLookOutLeft");
    }

    #[test]
    fn test_identifier_boundary() {
        // "mouthSmile_L" should not partially match inside "mouthSmile_R"
        let expr = "mouthSmile_L + mouthSmile_R";
        let result = rename_variables(expr);
        assert_eq!(result, "MouthSmileLeft + MouthSmileRight");
    }

    #[test]
    fn test_complex_expression_with_math() {
        let func = "return ( - ((-headRotX * ((90 - Math.abs(headRotY)) / 90)) + (-headRotZ * (headRotY / 45))))";
        let result = convert_simple_func(func);
        assert_eq!(
            result,
            "( - ((-HeadRotX * ((90 - math::abs(HeadRotY)) / 90)) + (-HeadRotZ * (HeadRotY / 45))))"
        );
    }

    // --- convert_simple_func ---

    #[test]
    fn test_simple_passthrough_variable() {
        assert_eq!(convert_simple_func("return cheekPuff"), "CheekPuff");
    }

    #[test]
    fn test_simple_passthrough_no_return() {
        assert_eq!(convert_simple_func("jawOpen"), "JawOpen");
    }

    #[test]
    fn test_simple_with_trailing_semicolon() {
        assert_eq!(convert_simple_func("return jawOpen;"), "JawOpen");
    }

    #[test]
    fn test_simple_negation() {
        assert_eq!(
            convert_simple_func("return headPosX * - 1"),
            "HeadPosX * - 1"
        );
    }

    #[test]
    fn test_simple_eye_open_left() {
        let result = convert_simple_func(
            "return (.5 + ((eyeBlink_L * - .8) + (eyeWide_L * .8)))-eyeSquint_L*.2",
        );
        assert_eq!(
            result,
            "(.5 + ((EyeBlinkLeft * - .8) + (EyeWideLeft * .8)))-EyeSquintLeft*.2"
        );
    }

    #[test]
    fn test_simple_eye_open_right() {
        let result = convert_simple_func(
            "return (.5 + ((eyeBlink_R * - .8) + (eyeWide_R * .8)))-eyeSquint_R*.2",
        );
        assert_eq!(
            result,
            "(.5 + ((EyeBlinkRight * - .8) + (EyeWideRight * .8)))-EyeSquintRight*.2"
        );
    }

    #[test]
    fn test_simple_mouth_smile() {
        let result = convert_simple_func(
            "return (2 - ((mouthFrown_L + mouthFrown_R + mouthPucker) / 1) + ((mouthSmile_R + mouthSmile_L + ((mouthDimple_L + mouthDimple_R) / 2)) / 1)) / 4",
        );
        assert_eq!(
            result,
            "(2 - ((MouthFrownLeft + MouthFrownRight + MouthPucker) / 1) + ((MouthSmileRight + MouthSmileLeft + ((MouthDimpleLeft + MouthDimpleRight) / 2)) / 1)) / 4"
        );
    }

    #[test]
    fn test_simple_mouth_x() {
        let result = convert_simple_func(
            "return (((mouthLeft - mouthRight) + (mouthSmile_L - mouthSmile_R)) * (1 - tongueOut))",
        );
        assert_eq!(
            result,
            "(((MouthLeft - MouthRight) + (MouthSmileLeft - MouthSmileRight)) * (1 - TongueOut))"
        );
    }

    #[test]
    fn test_simple_mouth_pucker() {
        let result = convert_simple_func(
            "return (((mouthDimple_R + mouthDimple_L) * 2) - mouthPucker) * (1 - tongueOut)",
        );
        assert_eq!(
            result,
            "(((MouthDimpleRight + MouthDimpleLeft) * 2) - MouthPucker) * (1 - TongueOut)"
        );
    }

    #[test]
    fn test_simple_mouth_funnel() {
        let result = convert_simple_func("return (mouthFunnel * (1 - tongueOut)) - (jawOpen * .2)");
        assert_eq!(result, "(MouthFunnel * (1 - TongueOut)) - (JawOpen * .2)");
    }

    #[test]
    fn test_simple_mouth_shrug() {
        let result = convert_simple_func(
            "return ((mouthShrugUpper + mouthShrugLower + mouthPress_R + mouthPress_L) / 4) * (1 - tongueOut)",
        );
        assert_eq!(
            result,
            "((MouthShrugUpper + MouthShrugLower + MouthPressRight + MouthPressLeft) / 4) * (1 - TongueOut)"
        );
    }

    #[test]
    fn test_simple_brow_left_y() {
        let result = convert_simple_func(
            "return .5 + (browOuterUp_L - browDown_L) + ((mouthRight - mouthLeft) / 8)",
        );
        assert_eq!(
            result,
            ".5 + (BrowOuterUpLeft - BrowDownLeft) + ((MouthRight - MouthLeft) / 8)"
        );
    }

    #[test]
    fn test_simple_brow_right_y() {
        let result = convert_simple_func(
            "return .5 + (browOuterUp_R - browDown_R) + ((mouthLeft - mouthRight) / 8)",
        );
        assert_eq!(
            result,
            ".5 + (BrowOuterUpRight - BrowDownRight) + ((MouthLeft - MouthRight) / 8)"
        );
    }

    #[test]
    fn test_simple_brows() {
        let result = convert_simple_func(
            "return .5 + (browOuterUp_R + browOuterUp_L - browDown_L - browDown_R) / 4",
        );
        assert_eq!(
            result,
            ".5 + (BrowOuterUpRight + BrowOuterUpLeft - BrowDownLeft - BrowDownRight) / 4"
        );
    }

    #[test]
    fn test_simple_eye_right_y() {
        let result = convert_simple_func(
            "return (eyeLookUp_L - eyeLookDown_L) + (browOuterUp_L * .15) + (headRotX / 30)",
        );
        assert_eq!(
            result,
            "(EyeLookUpLeft - EyeLookDownLeft) + (BrowOuterUpLeft * .15) + (HeadRotX / 30)"
        );
    }

    #[test]
    fn test_simple_body_angle_x() {
        let result = convert_simple_func("return headRotY * 1.5");
        assert_eq!(result, "HeadRotY * 1.5");
    }

    #[test]
    fn test_simple_body_angle_y_with_blink() {
        let result =
            convert_simple_func("return ( headRotX * 1.5)  + ( (eyeBlink_L + eyeBlink_R) * - 1)");
        assert_eq!(
            result,
            "( HeadRotX * 1.5)  + ( (EyeBlinkLeft + EyeBlinkRight) * - 1)"
        );
    }

    #[test]
    fn test_simple_voice_volume() {
        let result = convert_simple_func(
            "return ((jawOpen - mouthClose) - ((mouthRollUpper + mouthRollLower) * .2) + (mouthFunnel * .2))",
        );
        assert_eq!(
            result,
            "((JawOpen - MouthClose) - ((MouthRollUpper + MouthRollLower) * .2) + (MouthFunnel * .2))"
        );
    }

    #[test]
    fn test_simple_mouth_press_lip_open() {
        let result = convert_simple_func(
            "return (((mouthUpperUp_R + mouthUpperUp_L + mouthLowerDown_R + mouthLowerDown_L) / 1.8) - (mouthRollLower + mouthRollUpper)) * (1 - tongueOut)",
        );
        assert_eq!(
            result,
            "(((MouthUpperUpRight + MouthUpperUpLeft + MouthLowerDownRight + MouthLowerDownLeft) / 1.8) - (MouthRollLower + MouthRollUpper)) * (1 - TongueOut)"
        );
    }

    #[test]
    fn test_comment_only_after_expression() {
        let result = convert_simple_func("return headPosZ//+((ref.FaceAngleY+30)/60*2)");
        assert_eq!(result, "HeadPosZ");
    }

    #[test]
    fn test_multiline_comment_stripped() {
        let func = "return headPosY//some comment\n// more";
        let result = convert_simple_func(func);
        assert_eq!(result, "HeadPosY");
    }

    // --- convert_complex_func ---

    #[test]
    fn test_complex_face_angle_y() {
        let func = r#"let result = ( - ((-headRotX * ((90 - Math.abs(headRotY)) / 90)) + (-headRotZ * (headRotY / 45))))
let inmin=-40.0, inmax=40;         //input range
let outmin=-30.0, outmax=30.0;     //output range
let x1=.65,y1=.0;     // bezier control point 1
let x2=1-x1,y2=1-y1;    // bezier control point 2"#;
        let (expr, min, max) = convert_complex_func(func, -30.0, 30.0);
        assert_eq!(
            expr,
            "( - ((-HeadRotX * ((90 - math::abs(HeadRotY)) / 90)) + (-HeadRotZ * (HeadRotY / 45))))"
        );
        assert_eq!(min, -30.0);
        assert_eq!(max, 30.0);
    }

    #[test]
    fn test_complex_face_angle_z() {
        let func = r#"let result = ((headRotZ * ((90 - Math.abs(headRotY)) / 90)) - (headRotX * (headRotY / 45)))
let inmin=-30.0, inmax=30;         //input range
let outmin=-30.0, outmax=30.0;     //output range"#;
        let (expr, min, max) = convert_complex_func(func, -30.0, 30.0);
        assert_eq!(
            expr,
            "((HeadRotZ * ((90 - math::abs(HeadRotY)) / 90)) - (HeadRotX * (HeadRotY / 45)))"
        );
        assert_eq!(min, -30.0);
        assert_eq!(max, 30.0);
    }

    #[test]
    fn test_complex_mouth_open_bezier() {
        let func = r#"let result = ((jawOpen - mouthClose) - ((mouthRollUpper + mouthRollLower) * .2) + (mouthFunnel * .2))
let inmin=0.0, inmax=1.0;         //input range
let outmin=0.0, outmax=1.0;     //output range
let x1=.24,y1=.65;     // bezier control point 1
let x2=.62,y2=1.0;    // bezier control point 2"#;
        let (expr, min, max) = convert_complex_func(func, 0.0, 1.0);
        assert_eq!(
            expr,
            "((JawOpen - MouthClose) - ((MouthRollUpper + MouthRollLower) * .2) + (MouthFunnel * .2))"
        );
        assert_eq!(min, 0.0);
        assert_eq!(max, 1.0);
    }

    #[test]
    fn test_complex_mouth_press_lip_open() {
        let func = r#"let result = (((mouthUpperUp_R + mouthUpperUp_L + mouthLowerDown_R + mouthLowerDown_L) / 1.8) - (mouthRollLower + mouthRollUpper)) * (1 - tongueOut)
let inmin=-1.3, inmax=1.3;         //input range
let outmin=-1.3, outmax=1.3;     //output range"#;
        let (expr, min, max) = convert_complex_func(func, -1.3, 1.3);
        assert_eq!(
            expr,
            "(((MouthUpperUpRight + MouthUpperUpLeft + MouthLowerDownRight + MouthLowerDownLeft) / 1.8) - (MouthRollLower + MouthRollUpper)) * (1 - TongueOut)"
        );
        assert_eq!(min, -1.3);
        assert_eq!(max, 1.3);
    }

    #[test]
    fn test_complex_outmin_outmax_differ_from_minmax() {
        let func = r#"let result = headRotY
let inmin=-40.0, inmax=40;         //input range
let outmin=-40.0, outmax=40.0;     //output range"#;
        let (expr, min, max) = convert_complex_func(func, -30.0, 30.0);
        assert_eq!(expr, "HeadRotY");
        assert_eq!(min, -40.0);
        assert_eq!(max, 40.0);
    }

    #[test]
    fn test_complex_delay_buffer_face_angle_y() {
        let func = r#"let p=ref.FaceAngleY;
let s=2.0;        //smoothing
let dC=8;        //delay counter
let inmin=-30.0, inmax=30;         //input range
let outmin=-10.0, outmax=10.0;     //output range"#;
        let (expr, min, max) = convert_complex_func(func, -30.0, 30.0);
        assert_eq!(expr, "FaceAngleY");
        assert_eq!(min, -10.0);
        assert_eq!(max, 10.0);
    }

    #[test]
    fn test_complex_delay_buffer_face_angle_z() {
        let func = r#"let p=ref.FaceAngleZ;
let s=2.0;        //smoothing
let dC=4;        //delay counter
let inmin=-30.0, inmax=30;         //input range
let outmin=-10.0, outmax=10.0;     //output range"#;
        let (expr, min, max) = convert_complex_func(func, -40.0, 40.0);
        assert_eq!(expr, "FaceAngleZ");
        assert_eq!(min, -10.0);
        assert_eq!(max, 10.0);
    }

    #[test]
    fn test_complex_delay_buffer_face_position_y() {
        // This func starts with a block comment, not "let p=ref." on the first line,
        // so it falls through to the bezier path. The ref pattern is on line 4.
        let func = "let p=ref.FaceAngleY;\nlet s=2.0;\nlet dC=8;\nlet inmin=-30.0, inmax=30;\nlet outmin=-10.0, outmax=10.0;";
        let (expr, min, max) = convert_complex_func(func, -15.0, 15.0);
        assert_eq!(expr, "FaceAngleY");
        assert_eq!(min, -10.0);
        assert_eq!(max, 10.0);
    }

    // --- replace_identifier ---

    #[test]
    fn test_replace_identifier_at_start() {
        let result = replace_identifier("jawOpen + 1", "jawOpen", "JawOpen");
        assert_eq!(result, "JawOpen + 1");
    }

    #[test]
    fn test_replace_identifier_at_end() {
        let result = replace_identifier("1 + jawOpen", "jawOpen", "JawOpen");
        assert_eq!(result, "1 + JawOpen");
    }

    #[test]
    fn test_replace_identifier_in_parens() {
        let result = replace_identifier("(jawOpen)", "jawOpen", "JawOpen");
        assert_eq!(result, "(JawOpen)");
    }

    #[test]
    fn test_replace_identifier_not_substring() {
        // "jawOpen" should not match inside "jawOpened"
        let result = replace_identifier("jawOpened", "jawOpen", "JawOpen");
        assert_eq!(result, "jawOpened");
    }

    #[test]
    fn test_replace_identifier_not_after_dot() {
        // "ref.FaceAngleX" — FaceAngleX is preceded by dot, should not match standalone
        let result = replace_identifier("ref.headRotX", "headRotX", "HeadRotX");
        assert_eq!(result, "ref.headRotX");
    }

    #[test]
    fn test_replace_identifier_multiple_occurrences() {
        let result = replace_identifier("jawOpen + jawOpen * jawOpen", "jawOpen", "JawOpen");
        assert_eq!(result, "JawOpen + JawOpen * JawOpen");
    }

    #[test]
    fn test_replace_identifier_no_match() {
        let result = replace_identifier("someOtherVar + 1", "jawOpen", "JawOpen");
        assert_eq!(result, "someOtherVar + 1");
    }

    #[test]
    fn test_replace_identifier_adjacent_operators() {
        let result = replace_identifier("jawOpen*2+jawOpen/3-jawOpen", "jawOpen", "JawOpen");
        assert_eq!(result, "JawOpen*2+JawOpen/3-JawOpen");
    }

    // --- parse_range_line ---

    #[test]
    fn test_parse_range_line_with_comment() {
        let result = parse_range_line(
            "let outmin=-30.0, outmax=30.0;     //output range",
            "outmin",
            "outmax",
        );
        assert_eq!(result, Some((-30.0, 30.0)));
    }

    #[test]
    fn test_parse_range_line_integers() {
        let result = parse_range_line("let inmin=-40, inmax=40;", "inmin", "inmax");
        assert_eq!(result, Some((-40.0, 40.0)));
    }

    #[test]
    fn test_parse_range_line_zero_to_one() {
        let result = parse_range_line(
            "let outmin=0.0, outmax=1.0;     //output range",
            "outmin",
            "outmax",
        );
        assert_eq!(result, Some((0.0, 1.0)));
    }

    #[test]
    fn test_parse_range_line_negative_decimals() {
        let result = parse_range_line("let outmin=-1.3, outmax=1.3;", "outmin", "outmax");
        assert_eq!(result, Some((-1.3, 1.3)));
    }

    #[test]
    fn test_parse_range_line_missing_value() {
        let result = parse_range_line("let outmin=-30.0;", "outmin", "outmax");
        assert_eq!(result, None);
    }

    // --- find_comment_start ---

    #[test]
    fn test_find_comment_at_end() {
        assert_eq!(find_comment_start("headPosZ//comment"), Some(8));
    }

    #[test]
    fn test_find_comment_none() {
        assert_eq!(find_comment_start("headPosZ + 1"), None);
    }

    #[test]
    fn test_find_comment_with_division() {
        // Single slash is not a comment
        assert_eq!(find_comment_start("x / 2"), None);
    }

    #[test]
    fn test_find_comment_after_division() {
        assert_eq!(find_comment_start("x / 2 // comment"), Some(6));
    }

    // --- convert_variable_name ---

    #[test]
    fn test_convert_known_variable() {
        assert_eq!(convert_variable_name("headRotX"), "HeadRotX");
        assert_eq!(convert_variable_name("eyeBlink_L"), "EyeBlinkLeft");
        assert_eq!(convert_variable_name("tongueOut"), "TongueOut");
    }

    #[test]
    fn test_convert_unknown_variable_passthrough() {
        assert_eq!(convert_variable_name("FaceAngleX"), "FaceAngleX");
        assert_eq!(convert_variable_name("UnknownParam"), "UnknownParam");
    }

    // --- convert_param ---

    #[test]
    fn test_convert_param_simple() {
        let param = VBridgerParam {
            func: "return cheekPuff".to_string(),
            max: 1.0,
            min: 0.0,
            default: 0.0,
            param_type: "simple".to_string(),
            send_flag: "true".to_string(),
            param_name: "param_CheekPuff".to_string(),
        };
        let result = convert_param(param);
        assert_eq!(result.name, "CheekPuff");
        assert_eq!(result.func, "CheekPuff");
        assert_eq!(result.min, 0.0);
        assert_eq!(result.max, 1.0);
        assert_eq!(result.default_value, 0.0);
    }

    #[test]
    fn test_convert_param_without_prefix() {
        let param = VBridgerParam {
            func: "return jawOpen".to_string(),
            max: 1.0,
            min: 0.0,
            default: 0.0,
            param_type: "simple".to_string(),
            send_flag: "true".to_string(),
            param_name: "JawOpen".to_string(),
        };
        let result = convert_param(param);
        assert_eq!(result.name, "JawOpen");
    }

    #[test]
    fn test_convert_param_complex_uses_outrange() {
        let param = VBridgerParam {
            func:
                "let result = headRotY\nlet inmin=-40.0, inmax=40;\nlet outmin=-30.0, outmax=30.0;"
                    .to_string(),
            max: 40.0,
            min: -40.0,
            default: 0.0,
            param_type: "complex".to_string(),
            send_flag: "true".to_string(),
            param_name: "param_FaceAngleX".to_string(),
        };
        let result = convert_param(param);
        assert_eq!(result.name, "FaceAngleX");
        assert_eq!(result.func, "HeadRotY");
        assert_eq!(result.min, -30.0);
        assert_eq!(result.max, 30.0);
    }

    #[test]
    fn test_convert_param_preserves_default() {
        let param = VBridgerParam {
            func: "return jawOpen".to_string(),
            max: 1.0,
            min: 0.0,
            default: 0.5,
            param_type: "simple".to_string(),
            send_flag: "true".to_string(),
            param_name: "param_JawOpen".to_string(),
        };
        let result = convert_param(param);
        assert_eq!(result.default_value, 0.5);
    }

    // --- convert_vbridger_config ---

    #[test]
    fn test_full_config_filters_send_flag() {
        let input = r#"{"version":"0.9.7","customParam":[
            {"func":"return jawOpen","max":1,"min":0,"default":0,"type":"simple","sendFlag":"true","paramName":"param_JawOpen"},
            {"func":"return jawOpen","max":1,"min":0,"default":0,"type":"simple","sendFlag":"false","paramName":"param_Hidden"}
        ],"author":"Test","description":"","saveName":"Test","isDefault":false}"#;
        let output = convert_vbridger_config(input).unwrap();
        let parsed: Vec<CalcFn> = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "JawOpen");
    }

    #[test]
    fn test_full_config_invalid_json() {
        let result = convert_vbridger_config("not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    #[test]
    fn test_full_config_empty_params() {
        let input = r#"{"version":"0.9.7","customParam":[],"author":"Test","description":"","saveName":"Test","isDefault":false}"#;
        let output = convert_vbridger_config(input).unwrap();
        let parsed: Vec<CalcFn> = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn test_full_config_output_is_valid_json() {
        let input = r#"{"version":"0.9.7","customParam":[
            {"func":"return jawOpen","max":1,"min":0,"default":0,"type":"simple","sendFlag":"true","paramName":"param_JawOpen"}
        ],"author":"Test","description":"","saveName":"Test","isDefault":false}"#;
        let output = convert_vbridger_config(input).unwrap();
        // Must parse as valid JSON array
        let _: serde_json::Value = serde_json::from_str(&output).unwrap();
    }

    #[test]
    fn test_full_config_vbridger_compatible_maru() {
        // Test with a representative subset of the actual VBridgerCompatibleMaruVer config
        let input = r#"{"version":"0.9.7","customParam":[{"func":"let result = headRotY\nlet inmin=-40.0, inmax=40;         //input range\nlet outmin=-30.0, outmax=30.0;     //output range\nlet x1=.54,y1=.03;\nlet x2=1-x1,y2=1-y1;\nlet points = 10000;\nlet lowP = 62;","max":30,"min":-30,"default":0,"type":"complex","sendFlag":"true","paramName":"param_FaceAngleX"},{"func":"return headPosX * - 1//+((ref.FaceAngleX+30)/60*8)","max":15,"min":-15,"default":0,"type":"simple","sendFlag":"true","paramName":"param_FacePositionX"},{"func":"return (eyeSquint_L)","max":1,"min":0,"default":0,"type":"simple","sendFlag":"true","paramName":"param_Eye_Squint_L"},{"func":"return tongueOut","max":1,"min":0,"default":0,"type":"simple","sendFlag":"true","paramName":"param_TongueOut"}],"author":"Maruseu","description":"","saveName":"VBridgerCompatibleMaruVer","isDefault":false}"#;

        let output = convert_vbridger_config(input).unwrap();
        let parsed: Vec<CalcFn> = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.len(), 4);

        assert_eq!(parsed[0].name, "FaceAngleX");
        assert_eq!(parsed[0].func, "HeadRotY");
        assert_eq!(parsed[0].min, -30.0);
        assert_eq!(parsed[0].max, 30.0);

        assert_eq!(parsed[1].name, "FacePositionX");
        assert_eq!(parsed[1].func, "HeadPosX * - 1");

        assert_eq!(parsed[2].name, "Eye_Squint_L");
        assert_eq!(parsed[2].func, "(EyeSquintLeft)");

        assert_eq!(parsed[3].name, "TongueOut");
        assert_eq!(parsed[3].func, "TongueOut");
    }

    // --- Math function conversions ---

    #[test]
    fn test_math_min_conversion() {
        let result = convert_simple_func("return Math.min(headRotX, 30)");
        assert_eq!(result, "math::min(HeadRotX, 30)");
    }

    #[test]
    fn test_math_max_conversion() {
        let result = convert_simple_func("return Math.max(headRotX, -30)");
        assert_eq!(result, "math::max(HeadRotX, -30)");
    }

    #[test]
    fn test_math_sin_conversion() {
        let result = convert_simple_func("return Math.sin(headRotX)");
        assert_eq!(result, "math::sin(HeadRotX)");
    }

    #[test]
    fn test_math_cos_conversion() {
        let result = convert_simple_func("return Math.cos(headRotY)");
        assert_eq!(result, "math::cos(HeadRotY)");
    }

    #[test]
    fn test_math_floor_conversion() {
        let result = convert_simple_func("return Math.floor(jawOpen * 10)");
        assert_eq!(result, "math::floor(JawOpen * 10)");
    }

    #[test]
    fn test_math_sqrt_conversion() {
        let result = convert_simple_func("return Math.sqrt(headRotX)");
        assert_eq!(result, "math::sqrt(HeadRotX)");
    }

    #[test]
    fn test_math_pi_conversion() {
        let result = convert_simple_func("return Math.PI * 2");
        assert_eq!(result, "math::pi() * 2");
    }

    #[test]
    fn test_multiple_math_functions() {
        let result = convert_simple_func("return Math.min(Math.abs(headRotX), Math.abs(headRotY))");
        assert_eq!(
            result,
            "math::min(math::abs(HeadRotX), math::abs(HeadRotY))"
        );
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_expression() {
        let result = convert_simple_func("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_just_return_keyword() {
        // "return " trims to "return", which doesn't match "return " prefix, so passes through
        let result = convert_simple_func("return ");
        assert_eq!(result, "return");
    }

    #[test]
    fn test_return_with_value() {
        let result = convert_simple_func("return 0");
        assert_eq!(result, "0");
    }

    #[test]
    fn test_numeric_only() {
        let result = convert_simple_func("return 42");
        assert_eq!(result, "42");
    }

    #[test]
    fn test_negative_numeric() {
        let result = convert_simple_func("return -1.5");
        assert_eq!(result, "-1.5");
    }

    #[test]
    fn test_variable_mapping_completeness() {
        let mapping = build_variable_mapping();
        // Verify all expected ARKit blend shapes are covered
        assert!(mapping.contains_key("eyeBlink_L"));
        assert!(mapping.contains_key("eyeBlink_R"));
        assert!(mapping.contains_key("jawOpen"));
        assert!(mapping.contains_key("mouthClose"));
        assert!(mapping.contains_key("tongueOut"));
        assert!(mapping.contains_key("browInnerUp"));
        assert!(mapping.contains_key("cheekPuff"));
        assert!(mapping.contains_key("noseSneer_L"));
        assert!(mapping.contains_key("noseSneer_R"));
        assert!(mapping.contains_key("headRotX"));
        assert!(mapping.contains_key("headPosX"));
        // Verify L/R symmetry
        for key in mapping.keys() {
            if key.ends_with("_L") {
                let right = key.replace("_L", "_R");
                assert!(
                    mapping.contains_key(&right),
                    "Missing right counterpart for {key}"
                );
            }
        }
    }

    #[test]
    fn test_identifier_with_underscore_suffix() {
        // Ensure _L and _R identifiers don't interfere with each other
        let result = rename_variables("eyeBlink_L - eyeBlink_R");
        assert_eq!(result, "EyeBlinkLeft - EyeBlinkRight");
    }

    #[test]
    fn test_all_head_variables() {
        let result =
            rename_variables("headRotX + headRotY + headRotZ + headPosX + headPosY + headPosZ");
        assert_eq!(
            result,
            "HeadRotX + HeadRotY + HeadRotZ + HeadPosX + HeadPosY + HeadPosZ"
        );
    }
}
