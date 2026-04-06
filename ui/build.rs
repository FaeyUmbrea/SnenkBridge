use std::process::Command;

fn main() {
    slint_build::compile("src/app.slint").unwrap();

    #[cfg(windows)]
    embed_resource::compile("embed_resources.rc");

    generate_credits();
}

fn generate_credits() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = std::path::Path::new(&out_dir).join("credits.rs");

    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps"])
        .output();

    // Also get full dependency tree
    let deps_output = Command::new("cargo")
        .args(["metadata", "--format-version=1"])
        .output();

    let mut credits = Vec::new();

    if let Ok(output) = deps_output {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(packages) = json["packages"].as_array() {
                let mut seen = std::collections::HashSet::new();
                for pkg in packages {
                    let name = pkg["name"].as_str().unwrap_or("");
                    let version = pkg["version"].as_str().unwrap_or("");
                    let license = pkg["license"].as_str().unwrap_or("Unknown");
                    let key = format!("{name}-{version}");
                    if !seen.contains(&key)
                        && !name.starts_with("snenk_bridge")
                        && !name.is_empty()
                    {
                        seen.insert(key);
                        credits.push((name.to_string(), version.to_string(), license.to_string()));
                    }
                }
            }
        }
    }

    credits.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut lines = Vec::new();
    for (name, version, license) in &credits {
        lines.push(format!("{name} {version} ({license})"));
    }

    let credits_str = lines.join("\n");
    let code = format!(
        "pub const DEPENDENCY_CREDITS: &str = r#\"{}\"#;\n",
        credits_str
    );

    std::fs::write(&dest, code).unwrap();
}
