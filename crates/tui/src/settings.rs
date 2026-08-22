//! Persist user settings between runs: the locale, the preferred provider, and per-provider
//! model overrides. Only non-secret values live here — API keys are held in the OS keyring, never
//! in this file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The persisted settings file contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub lang: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, String>,
}

/// The settings together with the file they came from, so a caller never has to re-derive the
/// path (and a test can point it somewhere harmless).
pub(crate) struct SettingsHandle {
    pub settings: Settings,
    pub path: PathBuf,
}

impl SettingsHandle {
    /// Load from the default location, falling back to defaults when the file is absent.
    pub fn load() -> Self {
        let path = path();
        let settings = load_at(&path).unwrap_or_default();
        Self { settings, path }
    }
}

/// Location of the settings file: `$XDG_CONFIG_HOME/light-factory/config.json`
/// (falling back to `~/.config/light-factory/config.json`).
pub(crate) fn path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .or_else(|_| std::env::var("HOME").map(|home| format!("{home}/.config")))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base)
        .join("light-factory")
        .join("config.json")
}

/// Load the settings stored at `path`, or `None` when the file is missing, unreadable, or
/// malformed — the caller falls back to defaults in every case.
pub(crate) fn load_at(path: &Path) -> Option<Settings> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Settings>(&raw).ok()
}

/// Persist the settings to `path`, creating the parent directory as needed.
pub(crate) fn save_at(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "light-factory-settings-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn round_trips_locale() {
        let path = temp("lang");
        let settings = Settings {
            lang: "es".to_string(),
            provider: None,
            models: BTreeMap::new(),
        };
        save_at(&path, &settings).unwrap();
        assert_eq!(load_at(&path).unwrap().lang, "es");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn round_trips_provider_and_models() {
        let path = temp("provider");
        let settings = Settings {
            lang: "en".to_string(),
            provider: Some("openai".to_string()),
            models: BTreeMap::from([("openai".to_string(), "gpt-5".to_string())]),
        };
        save_at(&path, &settings).unwrap();
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.provider, Some("openai".to_string()));
        assert_eq!(loaded.models.get("openai"), Some(&"gpt-5".to_string()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_legacy_lang_only_file_loads_without_preferences() {
        let path = temp("legacy");
        fs::write(&path, "{\"lang\":\"es\"}").unwrap();
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.lang, "es");
        assert_eq!(loaded.provider, None);
        assert!(loaded.models.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_loads_none() {
        let path = temp("none");
        assert!(load_at(&path).is_none());
    }

    #[test]
    fn malformed_file_loads_none() {
        let path = temp("bad");
        fs::write(&path, "{ not json").unwrap();
        assert!(load_at(&path).is_none());
        let _ = fs::remove_file(&path);
    }
}
