use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::vitamins::CalcFn;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnekPreset {
    pub format: String,
    pub version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub params: Vec<CalcFn>,
}

impl SnekPreset {
    pub fn new(title: String, params: Vec<CalcFn>) -> Self {
        Self {
            format: "snek".to_string(),
            version: 1,
            title,
            author: String::new(),
            description: String::new(),
            params,
        }
    }
}

/// Sanitize a preset title into a safe filename (without extension).
/// Lowercase, replace spaces with hyphens, strip non-alphanumeric except hyphens/underscores.
/// Empty input returns "preset".
pub fn sanitize_title(title: &str) -> String {
    let lowered = title.to_lowercase();
    let mut result = String::new();
    for ch in lowered.chars() {
        if ch == ' ' || ch == '\\' {
            result.push('-');
        } else if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            result.push(ch);
        }
        // strip anything else (including '/')
    }
    // Trim leading/trailing hyphens
    let trimmed = result.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "preset".to_string()
    } else {
        trimmed
    }
}

/// Save a preset to a directory as a .snek file.
/// Filename derived from title. If a file with the same title already exists, overwrite it.
/// If a file with the same filename but different title exists, append a number.
/// Returns the filename used.
pub fn save_preset(dir: &Path, preset: &SnekPreset) -> Result<String, String> {
    let base = sanitize_title(&preset.title);

    // Check if any existing file has this exact title — overwrite it.
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("snek") {
                if let Ok(existing) = load_preset(&path) {
                    if existing.title == preset.title {
                        let filename = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        let json = serde_json::to_string_pretty(preset)
                            .map_err(|e| e.to_string())?;
                        fs::write(&path, json).map_err(|e| e.to_string())?;
                        return Ok(filename);
                    }
                }
            }
        }
    }

    // No existing file with the same title; find a free filename.
    let candidate = format!("{base}.snek");
    let path = dir.join(&candidate);
    if !path.exists() {
        let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        return Ok(candidate);
    }

    // Filename taken by a different title — append a number.
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}-{n}.snek");
        let path = dir.join(&candidate);
        if !path.exists() {
            let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
            fs::write(&path, json).map_err(|e| e.to_string())?;
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Load a preset from a JSON string. Same logic as load_preset but from a string.
pub fn load_from_str(json: &str) -> Result<SnekPreset, String> {
    // Try parsing as a bare CalcFn array first.
    if let Ok(params) = serde_json::from_str::<Vec<CalcFn>>(json) {
        return Ok(SnekPreset {
            format: "snek".to_string(),
            version: 1,
            title: String::new(),
            author: String::new(),
            description: String::new(),
            params,
        });
    }

    // Parse as a snek envelope.
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| e.to_string())?;

    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    match version {
        1 => {
            let preset: SnekPreset =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(preset)
        }
        v => Err(format!(
            "This preset was created with a newer version of SnenkBridge (format version {v})"
        )),
    }
}

/// Load a preset from a file. Handles both .snek envelopes and bare CalcFn JSON arrays.
/// For bare arrays: returns SnekPreset with empty title/author/description.
/// For .snek files: checks version field, dispatches to versioned loader.
/// Unknown versions return an error.
pub fn load_preset(path: &Path) -> Result<SnekPreset, String> {
    let json = fs::read_to_string(path).map_err(|e| e.to_string())?;
    load_from_str(&json)
}

/// List all .snek presets in a directory, sorted alphabetically by title (case-insensitive).
pub fn list_presets(dir: &Path) -> Vec<SnekPreset> {
    let mut presets: Vec<SnekPreset> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "snek")
                .unwrap_or(false)
        })
        .filter_map(|e| load_preset(&e.path()).ok())
        .collect();

    presets.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    presets
}

/// Delete a .snek file from a directory by filename.
pub fn delete_preset(dir: &Path, filename: &str) -> Result<(), String> {
    let path = dir.join(filename);
    fs::remove_file(&path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_calc_fn(name: &str) -> CalcFn {
        CalcFn {
            name: name.to_string(),
            func: "x".to_string(),
            min: 0.0,
            max: 1.0,
            default_value: 0.5,
            delay_buffer: None,
        }
    }

    #[test]
    fn round_trip_serialization() {
        let params = vec![make_calc_fn("ParamA"), make_calc_fn("ParamB")];
        let preset = SnekPreset {
            format: "snek".to_string(),
            version: 1,
            title: "My Preset".to_string(),
            author: "Alice".to_string(),
            description: "A test preset".to_string(),
            params,
        };

        let json = serde_json::to_string(&preset).expect("serialization failed");
        let restored: SnekPreset = serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(restored.format, preset.format);
        assert_eq!(restored.version, preset.version);
        assert_eq!(restored.title, preset.title);
        assert_eq!(restored.author, preset.author);
        assert_eq!(restored.description, preset.description);
        assert_eq!(restored.params.len(), preset.params.len());
        assert_eq!(restored.params[0].name, preset.params[0].name);
        assert_eq!(restored.params[1].name, preset.params[1].name);
    }

    #[test]
    fn empty_author_and_description_omitted_from_json() {
        let preset = SnekPreset::new("Minimal".to_string(), vec![make_calc_fn("ParamX")]);

        assert!(preset.author.is_empty());
        assert!(preset.description.is_empty());

        let json = serde_json::to_string(&preset).expect("serialization failed");

        assert!(
            !json.contains("\"author\""),
            "author should be omitted when empty, but got: {json}"
        );
        assert!(
            !json.contains("\"description\""),
            "description should be omitted when empty, but got: {json}"
        );

        // Verify the rest still round-trips correctly
        let restored: SnekPreset = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(restored.title, "Minimal");
        assert_eq!(restored.author, "");
        assert_eq!(restored.description, "");
        assert_eq!(restored.params.len(), 1);
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().expect("tempdir failed");
        let preset = SnekPreset {
            format: "snek".to_string(),
            version: 1,
            title: "My Test Preset".to_string(),
            author: "Tester".to_string(),
            description: "A description".to_string(),
            params: vec![make_calc_fn("Alpha"), make_calc_fn("Beta")],
        };

        let filename = save_preset(dir.path(), &preset).expect("save failed");
        assert!(filename.ends_with(".snek"));

        let loaded = load_preset(&dir.path().join(&filename)).expect("load failed");
        assert_eq!(loaded.title, preset.title);
        assert_eq!(loaded.author, preset.author);
        assert_eq!(loaded.description, preset.description);
        assert_eq!(loaded.params.len(), 2);
        assert_eq!(loaded.params[0].name, "Alpha");
        assert_eq!(loaded.params[1].name, "Beta");
    }

    #[test]
    fn test_list_presets() {
        let dir = tempdir().expect("tempdir failed");

        let preset_z = SnekPreset::new("Zebra Preset".to_string(), vec![make_calc_fn("P1")]);
        let preset_a = SnekPreset::new("Apple Preset".to_string(), vec![make_calc_fn("P2")]);

        save_preset(dir.path(), &preset_z).expect("save z failed");
        save_preset(dir.path(), &preset_a).expect("save a failed");

        let list = list_presets(dir.path());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "Apple Preset");
        assert_eq!(list[1].title, "Zebra Preset");
    }

    #[test]
    fn test_delete_preset() {
        let dir = tempdir().expect("tempdir failed");
        let preset = SnekPreset::new("DeleteMe".to_string(), vec![make_calc_fn("P")]);

        let filename = save_preset(dir.path(), &preset).expect("save failed");
        let path = dir.path().join(&filename);
        assert!(path.exists());

        delete_preset(dir.path(), &filename).expect("delete failed");
        assert!(!path.exists());
    }

    #[test]
    fn test_load_bare_json_array() {
        let dir = tempdir().expect("tempdir failed");
        let bare_json = r#"[{"name":"Gain","func":"x","min":0.0,"max":1.0,"defaultValue":0.5}]"#;
        let path = dir.path().join("bare.snek");
        fs::write(&path, bare_json).expect("write failed");

        let loaded = load_preset(&path).expect("load failed");
        assert_eq!(loaded.title, "");
        assert_eq!(loaded.author, "");
        assert_eq!(loaded.description, "");
        assert_eq!(loaded.params.len(), 1);
        assert_eq!(loaded.params[0].name, "Gain");
    }

    #[test]
    fn test_load_unknown_version_errors() {
        let dir = tempdir().expect("tempdir failed");
        let json = r#"{"format":"snek","version":99,"title":"Future","params":[]}"#;
        let path = dir.path().join("future.snek");
        fs::write(&path, json).expect("write failed");

        let result = load_preset(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("99"),
            "error should mention version 99, got: {msg}"
        );
        assert!(
            msg.contains("newer version"),
            "error should mention newer version, got: {msg}"
        );
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_title("My Cool Preset!"), "my-cool-preset");
        assert_eq!(sanitize_title("hello/world\\test"), "helloworld-test");
        assert_eq!(sanitize_title("  spaces  "), "spaces");
        assert_eq!(sanitize_title(""), "preset");
    }
}
