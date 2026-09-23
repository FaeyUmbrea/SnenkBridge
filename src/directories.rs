use std::path::PathBuf;

/// Keep the application's existing locations, including Windows folder
/// redirection and the macOS Application Support directory.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        known_folders::get_known_folder_path(known_folders::KnownFolder::RoamingAppData)
    }
    #[cfg(not(windows))]
    {
        resolve_unix(
            std::env::home_dir(),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            cfg!(any(target_os = "macos", target_os = "ios")),
        )
    }
}

#[cfg(any(not(windows), test))]
fn resolve_unix(home: Option<PathBuf>, xdg: Option<PathBuf>, apple: bool) -> Option<PathBuf> {
    if apple {
        home.map(|home| home.join("Library/Application Support"))
    } else {
        xdg.filter(|path| path.is_absolute())
            .or_else(|| home.map(|home| home.join(".config")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_locations_preserve_overrides_and_fallbacks() {
        // These are Unix paths; Windows tests can still exercise the policy
        // using paths that are absolute on the host platform.
        let root = std::env::temp_dir();
        let home = root.join("home");
        let custom = root.join("custom");
        for xdg in [None, Some(PathBuf::new()), Some(PathBuf::from("relative"))] {
            assert_eq!(
                resolve_unix(Some(home.clone()), xdg.clone(), false),
                Some(home.join(".config"))
            );
            assert_eq!(resolve_unix(None, xdg, false), None);
        }
        for home in [None, Some(home.clone())] {
            assert_eq!(
                resolve_unix(home, Some(custom.clone()), false),
                Some(custom.clone())
            );
        }
        assert_eq!(
            resolve_unix(Some(home.clone()), Some(custom.clone()), true),
            Some(home.join("Library/Application Support"))
        );
        assert_eq!(resolve_unix(None, Some(custom), true), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_macos_location_is_application_support() {
        assert_eq!(
            config_dir(),
            std::env::home_dir().map(|home| home.join("Library/Application Support"))
        );
    }
}
