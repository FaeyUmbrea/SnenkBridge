use serde_json::Value;

use crate::model::{DelaySettings, Parameter, Preset};

use super::{convert::convert_expression, ImportResult};

pub fn without_blocks(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        result.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("*/") else {
            return result;
        };
        rest = &rest[start + 2 + end + 2..];
    }
    result.push_str(rest);
    result
}

pub fn assignment(text: &str, key: &str) -> Option<String> {
    for statement in text.split([';', '\n']).flat_map(|line| line.split(',')) {
        let statement = statement.trim();
        let statement = statement
            .strip_prefix("var ")
            .or_else(|| statement.strip_prefix("let "))
            .or_else(|| statement.strip_prefix("const "))
            .unwrap_or(statement);
        if let Some((name, value)) = statement.split_once('=') {
            if name.trim() == key {
                return Some(value.trim().to_owned());
            }
        }
    }
    None
}

pub fn import_vitamins(text: &str, swap_axes: bool) -> Result<ImportResult, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let items = root
        .get("customParam")
        .and_then(Value::as_array)
        .ok_or_else(|| "Vitamins customParam must be an array".to_string())?;
    let mut preset = Preset::new(
        root.get("saveName").and_then(Value::as_str).unwrap_or(""),
        Vec::new(),
    );
    preset.author = root
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    preset.description = root
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .into();
    let mut warnings = Vec::new();
    for item in items {
        if item.get("sendFlag").and_then(Value::as_str) != Some("true") {
            continue;
        }
        let string = |key| {
            item.get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Missing Vitamins {key}"))
        };
        let number = |key| {
            item.get(key)
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("Missing numeric Vitamins {key}"))
        };
        let original = string("paramName")?;
        let name = original.strip_prefix("param_").unwrap_or(original);
        let name = match name {
            "FaceAngleX" if swap_axes => "FaceAngleY",
            "FaceAngleY" if swap_axes => "FaceAngleX",
            "Eye_Squint_L" => "EyeSquintL",
            "Eye_Squint_R" => "EyeSquintR",
            _ => name,
        };
        let mut p = Parameter {
            name: name.into(),
            func: String::new(),
            min: number("min")?,
            max: number("max")?,
            default_value: number("default")?,
            delay_buffer: None,
        };
        let source = without_blocks(string("func")?);
        if let Some(reference) = assignment(&source, "ref").or_else(|| assignment(&source, "p")) {
            let read = |key, default| -> Result<f64, String> {
                assignment(&source, key)
                    .map(|v| {
                        v.parse::<f64>()
                            .map_err(|_| format!("{}: invalid {key}", p.name))
                    })
                    .unwrap_or(Ok(default))
            };
            let count = read("dC", 1.0)?;
            if !count.is_finite() || count < 0.0 || count.fract() != 0.0 || count > 1_000_000.0 {
                return Err(format!("{}: invalid delay count", p.name));
            }
            let reference = reference
                .trim_matches(['\'', '"'])
                .strip_prefix("ref.")
                .unwrap_or(reference.trim_matches(['\'', '"']))
                .to_owned();
            let d = DelaySettings {
                ref_param: reference,
                smoothing: read("s", 1.0)?,
                delay_count: count as usize,
                in_min: read("inmin", p.min)?,
                in_max: read("inmax", p.max)?,
                out_min: read("outmin", p.min)?,
                out_max: read("outmax", p.max)?,
            };
            p.min = d.out_min;
            p.max = d.out_max;
            if swap_axes && matches!(d.ref_param.as_str(), "FaceAngleX" | "FaceAngleY") {
                warnings.push(format!(
                    "{}: delayed reference {} is unchanged by axis swapping",
                    p.name, d.ref_param
                ));
            }
            p.delay_buffer = Some(d);
        } else if let Some(result) = assignment(&source, "result") {
            p.func = convert_expression(&result);
            for (key, target) in [("outmin", &mut p.min), ("outmax", &mut p.max)] {
                if let Some(value) = assignment(&source, key) {
                    *target = value
                        .parse()
                        .map_err(|_| format!("{}: invalid {key}", p.name))?;
                }
            }
            warnings.push(format!(
                "{}: easing curve omitted; underlying expression retained",
                p.name
            ));
        } else {
            p.func = convert_expression(&source);
            if source.contains(['{', '}'])
                || source.contains("var ")
                || source.contains("let ")
                || source.contains("const ")
            {
                warnings.push(format!(
                    "{}: unsupported complex expression requires editing",
                    p.name
                ));
                p.func.clear();
            }
        }
        preset.params.push(p);
    }
    warnings.extend(
        crate::evaluation::validation_errors(&preset.params)
            .into_iter()
            .filter(|e| !e.is_empty()),
    );
    Ok(ImportResult { preset, warnings })
}
