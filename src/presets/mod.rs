mod convert;
mod store;
mod vitamins;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use convert::convert_expression;
pub use store::PresetStore;
pub use vitamins::import_vitamins;

use serde_json::Value;

use crate::model::Preset;

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
