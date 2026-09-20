use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::model::Preset;

use super::{filename, parse, StoredPreset};

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
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let suffix = SERIAL.fetch_add(1, Ordering::Relaxed);
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
