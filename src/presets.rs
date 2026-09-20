use crate::model::{DelaySettings, Parameter, Preset};
use serde_json::Value;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub struct StoredPreset {
    pub filename: String,
    pub preset: Preset,
}
pub struct ImportResult {
    pub preset: Preset,
    pub warnings: Vec<String>,
}
pub fn parse(text: &str) -> Result<Preset, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let preset = if value.is_array() {
        Preset::new(
            "",
            serde_json::from_value(value).map_err(|e| e.to_string())?,
        )
    } else {
        serde_json::from_value::<Preset>(value).map_err(|e| e.to_string())?
    };
    if preset.format != "snek" {
        return Err("Unsupported preset format".into());
    }
    if preset.version != 1 {
        return Err(format!("Unsupported preset version {}", preset.version));
    }
    Ok(preset)
}
pub fn filename(title: &str) -> String {
    let s: String = title
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c == ' ' || c == '\\' {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    let s = s.trim_matches('-');
    format!("{}.snek", if s.is_empty() { "preset" } else { s })
}
pub struct PresetStore {
    directory: PathBuf,
}
impl PresetStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
    fn path(&self, name: &str) -> Result<PathBuf, String> {
        let mut parts = Path::new(name).components();
        if !matches!(parts.next(), Some(Component::Normal(_)))
            || parts.next().is_some()
            || !name.ends_with(".snek")
            || name.contains(['/', '\\'])
        {
            return Err("Invalid preset filename".into());
        }
        let path = self.directory.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Preset symlinks are not supported".into());
        }
        Ok(path)
    }
    pub fn list(&self) -> Result<Vec<StoredPreset>, String> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.directory)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Ok(preset) = self.load(&name) {
                found.push(StoredPreset {
                    filename: name,
                    preset,
                });
            }
        }
        found.sort_by_key(|p| p.preset.title.to_lowercase());
        Ok(found)
    }
    pub fn load(&self, name: &str) -> Result<Preset, String> {
        parse(&fs::read_to_string(self.path(name)?).map_err(|e| e.to_string())?)
    }
    pub fn delete(&self, name: &str) -> Result<(), String> {
        fs::remove_file(self.path(name)?).map_err(|e| e.to_string())
    }
    fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
        use std::io::Write;
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suffix = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = path.with_extension(format!("snek-{}-{}.tmp", std::process::id(), suffix));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|e| e.to_string())
    }
    pub fn save(&self, preset: &Preset) -> Result<String, String> {
        let text = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
        parse(&text)?;
        fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        if let Some(existing) = self
            .list()?
            .into_iter()
            .find(|e| e.preset.title == preset.title)
        {
            Self::write_atomic(&self.path(&existing.filename)?, &text)?;
            return Ok(existing.filename);
        }
        let base = filename(&preset.title);
        let mut n = 1;
        loop {
            let name = if n == 1 {
                base.clone()
            } else {
                format!("{}-{n}.snek", base.trim_end_matches(".snek"))
            };
            let path = self.path(&name)?;
            if path.exists() {
                n += 1;
                continue;
            }
            Self::write_atomic(&path, &text)?;
            return Ok(name);
        }
    }
}
fn mapping() -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    for name in [
        "headRotX",
        "headRotY",
        "headRotZ",
        "headPosX",
        "headPosY",
        "headPosZ",
        "browInnerUp",
        "cheekPuff",
        "jawForward",
        "jawLeft",
        "jawOpen",
        "jawRight",
        "mouthClose",
        "mouthFunnel",
        "mouthLeft",
        "mouthPucker",
        "mouthRight",
        "mouthRollLower",
        "mouthRollUpper",
        "mouthShrugLower",
        "mouthShrugUpper",
        "tongueOut",
    ] {
        m.insert(
            name.into(),
            format!("{}{}", name[..1].to_uppercase(), &name[1..]),
        );
    }
    for stem in [
        "eyeBlink",
        "eyeLookDown",
        "eyeLookIn",
        "eyeLookOut",
        "eyeLookUp",
        "eyeSquint",
        "eyeWide",
        "browDown",
        "browOuterUp",
        "cheekSquint",
        "mouthDimple",
        "mouthFrown",
        "mouthLowerDown",
        "mouthPress",
        "mouthSmile",
        "mouthStretch",
        "mouthUpperUp",
        "noseSneer",
    ] {
        for (short, long) in [("L", "Left"), ("R", "Right")] {
            m.insert(
                format!("{stem}_{short}"),
                format!("{}{}{long}", stem[..1].to_uppercase(), &stem[1..]),
            );
        }
    }
    m
}
pub fn convert_expression(source: &str) -> String {
    let source = source
        .trim()
        .strip_prefix("return ")
        .unwrap_or(source.trim());
    let source = source
        .split("//")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(';')
        .trim();
    let names = mapping();
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1
            }
            let name: String = chars[start..i].iter().collect();
            let eligible = start == 0
                || !(chars[start - 1].is_alphanumeric()
                    || chars[start - 1] == '_'
                    || chars[start - 1] == '.');
            if eligible && name == "Math" && i < chars.len() && chars[i] == '.' {
                let mut end = i + 1;
                while end < chars.len() && chars[end].is_alphanumeric() {
                    end += 1
                }
                let member: String = chars[i + 1..end].iter().collect();
                if member == "PI" {
                    out.push_str("math::pi()");
                    i = end;
                    continue;
                }
                if [
                    "abs", "min", "max", "sin", "cos", "floor", "ceil", "sqrt", "pow",
                ]
                .contains(&member.as_str())
                {
                    out.push_str("math::");
                    out.push_str(&member);
                    i = end;
                    continue;
                }
            }
            out.push_str(if eligible {
                names.get(&name).map(String::as_str).unwrap_or(&name)
            } else {
                &name
            });
        } else {
            out.push(chars[i]);
            i += 1
        }
    }
    out
}
fn without_blocks(text: &str) -> String {
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
fn assignment(text: &str, key: &str) -> Option<String> {
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
        .ok_or("Vitamins customParam must be an array")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_comma_declarations_and_reference_alias() {
        let text = r#"{"saveName":"Axes","customParam":[
            {"sendFlag":"true","paramName":"param_FaceAngleX","func":"let p=ref.FaceAngleX; let outmin=-30, outmax=30;","min":0,"max":1,"default":0}
        ]}"#;
        let result = import_vitamins(text, false).unwrap();
        let parameter = &result.preset.params[0];
        let delay = parameter.delay_buffer.as_ref().unwrap();
        assert_eq!(delay.ref_param, "FaceAngleX");
        assert_eq!(delay.out_min, -30.0);
        assert_eq!(delay.out_max, 30.0);
    }
}
