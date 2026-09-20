use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{json, Value};

use super::{fatal, Failure};

pub fn identity() -> Value {
    json!({"pluginName":"SnenkBridge","pluginDeveloper":"FaeyUmbrea"})
}

pub fn load_token(path: &Path) -> Result<Option<String>, String> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut token = String::new();
            file.take(65)
                .read_to_string(&mut token)
                .map_err(|e| format!("Cannot read authorization token: {e}"))?;
            if token.is_empty() || token.len() > 64 || !token.is_ascii() {
                return Ok(None);
            }
            Ok(Some(token))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read authorization token: {e}")),
    }
}

pub fn save_token(path: &Path, token: &str) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| fatal(format!("Cannot create token directory: {e}")))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let temp = path.with_extension(format!(
        "token-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temp)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| fatal(format!("Cannot save authorization token: {e}")))
}
