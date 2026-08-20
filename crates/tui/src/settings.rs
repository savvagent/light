//! Persist user settings (currently just the locale) between runs.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The persisted settings file contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    lang: String,
}

/// Location of the settings file: `$XDG_CONFIG_HOME/light-factory/config.json`
/// (falling back to `~/.config/light-factory/config.json`).
fn path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|home| format!("{home}/.config")))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base)
        .join("light-factory")
        .join("config.json")
}

/// Load the saved locale, if any and well-formed.
pub fn load_lang() -> Option<String> {
    load_lang_at(&path())
}

/// Persist the chosen locale, creating the config directory as needed.
pub fn save_lang(lang: &str) -> anyhow::Result<()> {
    save_lang_at(&path(), lang)
}

fn load_lang_at(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Settings>(&raw).ok().map(|s| s.lang)
}

fn save_lang_at(path: &Path, lang: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let settings = Settings {
        lang: lang.to_string(),
    };
    fs::write(path, serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_locale() {
        let path = std::env::temp_dir().join(format!(
            "light-factory-settings-{}.json",
            std::process::id()
        ));
        save_lang_at(&path, "es").unwrap();
        assert_eq!(load_lang_at(&path), Some("es".to_string()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_loads_none() {
        let path = std::env::temp_dir().join(format!(
            "light-factory-settings-none-{}.json",
            std::process::id()
        ));
        assert_eq!(load_lang_at(&path), None);
    }

    #[test]
    fn malformed_file_loads_none() {
        let path = std::env::temp_dir().join(format!(
            "light-factory-settings-bad-{}.json",
            std::process::id()
        ));
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_lang_at(&path), None);
        let _ = fs::remove_file(&path);
    }
}
