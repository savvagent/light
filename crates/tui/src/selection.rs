//! Compose the active provider from the environment, persisted preferences, and the OS keyring,
//! and expose the pieces the commands need (`key_status`, `rebuild`).

use std::sync::Arc;

use light_factory_providers::{
    Provider, Selection, build_provider, env_key_var, selection_from_env,
};
use light_factory_tui::credentials::CredentialStore;

use crate::provider::ProviderInfo;
use crate::settings::Settings;

/// The remote provider ids, in key-precedence order.
pub const REMOTE_IDS: [&str; 4] = ["anthropic", "openai", "gemini", "deepseek"];

/// Pure classification of a provider's key source.
fn classify(env_key: Option<String>, keyring_key: Option<String>) -> KeyStatus {
    if env_key.as_ref().is_some_and(|k| !k.is_empty()) {
        KeyStatus::Env
    } else if keyring_key.is_some() {
        KeyStatus::Keyring
    } else {
        KeyStatus::None
    }
}

/// Read a named environment variable. The single real-environment source for [`sources`] and
/// [`resolve_key`]; every other caller supplies its own reader.
fn process_env(var: &str) -> Option<String> {
    std::env::var(var).ok()
}

/// The env key and keyring key for a provider, resolved independently. `env` supplies the value
/// of a named environment variable; tests pass a stub so the ambient environment cannot decide
/// the outcome.
fn sources_with(
    provider: &str,
    store: &dyn CredentialStore,
    env: impl Fn(&str) -> Option<String>,
) -> (Option<String>, Option<String>) {
    let env_key = env_key_var(provider).and_then(env);
    let keyring_key = store.get(provider).ok().flatten();
    (env_key, keyring_key)
}

/// The env key and keyring key for a provider, read from the process environment.
fn sources(provider: &str, store: &dyn CredentialStore) -> (Option<String>, Option<String>) {
    sources_with(provider, store, process_env)
}

/// Where a provider's key comes from, for the `/key` listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    Env,
    Keyring,
    None,
}

/// Classify a provider's key source without revealing the value.
pub fn key_status(provider: &str, store: &dyn CredentialStore) -> KeyStatus {
    let (env_key, keyring_key) = sources(provider, store);
    classify(env_key, keyring_key)
}

/// Resolve a provider's API key: env wins over keyring; an empty env value is treated as absent
/// (mirrors `classify`'s empty-string rule) so the connect flow never fetches with an empty key.
/// Pure so it is unit-testable without the process env.
fn resolve_key_from(env_key: Option<String>, keyring_key: Option<String>) -> Option<String> {
    if let Some(key) = env_key.filter(|k| !k.is_empty()) {
        Some(key)
    } else {
        keyring_key
    }
}

/// The resolved API key for a provider against an explicit environment. Lets the wiring
/// (`env_key_var` naming plus `store.get`) be tested without the process env deciding the result.
fn resolve_key_with(
    provider: &str,
    store: &dyn CredentialStore,
    env: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let (env_key, keyring_key) = sources_with(provider, store, env);
    resolve_key_from(env_key, keyring_key)
}

/// The resolved API key for a provider (env over keyring), or `None` when no key is available.
pub fn resolve_key(provider: &str, store: &dyn CredentialStore) -> Option<String> {
    resolve_key_with(provider, store, process_env)
}

/// Layer the persisted preferences and keyring keys over an env-derived [`Selection`]. Pure and
/// testable: the caller supplies the base.
pub fn apply_preferences(
    mut base: Selection,
    settings: &Settings,
    store: &dyn CredentialStore,
) -> Selection {
    for id in REMOTE_IDS {
        if !base.keys.contains_key(id)
            && let Ok(Some(key)) = store.get(id)
        {
            base.keys.insert(id.to_string(), key);
        }
    }
    base.preferred = settings.provider.clone();
    for (id, model) in &settings.models {
        base.models
            .entry(id.clone())
            .or_insert_with(|| model.clone());
    }
    base
}

/// Assemble the effective [`Selection`]: environment (via the providers crate), then the keyring
/// and persisted preferences layered on top.
pub fn build_selection(settings: &Settings, store: &dyn CredentialStore) -> Selection {
    apply_preferences(selection_from_env(), settings, store)
}

/// Build the provider and its display record from an explicit [`Selection`]. Pure.
fn build_and_info(selection: &Selection) -> (Arc<dyn Provider>, ProviderInfo) {
    let built = build_provider(selection);
    let id = built.provider.id().to_string();
    let info = ProviderInfo {
        id,
        model: built.model,
        offline: built.offline,
        selected_by: built.selected_by,
        warnings: built.warnings,
    };
    (Arc::from(built.provider), info)
}

/// Build the active provider and its display record from the given settings and keyring.
pub fn rebuild(
    settings: &Settings,
    store: &dyn CredentialStore,
) -> (Arc<dyn Provider>, ProviderInfo) {
    build_and_info(&build_selection(settings, store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_factory_providers::SelectedBy;
    use light_factory_tui::credentials::MemStore;

    fn settings(provider: Option<&str>) -> Settings {
        Settings {
            lang: "en".to_string(),
            provider: provider.map(str::to_string),
            models: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn classify_distinguishes_env_keyring_and_none() {
        assert_eq!(classify(Some("k".to_string()), None), KeyStatus::Env);
        assert_eq!(
            classify(Some(String::new()), Some("k".to_string())),
            KeyStatus::Keyring
        );
        assert_eq!(classify(None, Some("k".to_string())), KeyStatus::Keyring);
        assert_eq!(classify(None, None), KeyStatus::None);
    }

    #[test]
    fn non_remote_providers_have_no_key() {
        let store = MemStore::new();
        assert_eq!(key_status("ollama", &store), KeyStatus::None);
        assert_eq!(key_status("local", &store), KeyStatus::None);
    }

    #[test]
    fn resolve_key_from_prefers_env_over_keyring() {
        assert_eq!(
            resolve_key_from(Some("env".to_string()), Some("ring".to_string())),
            Some("env".to_string())
        );
        assert_eq!(
            resolve_key_from(None, Some("ring".to_string())),
            Some("ring".to_string())
        );
        assert_eq!(resolve_key_from(None, None), None);
    }

    #[test]
    fn resolve_key_from_treats_an_empty_env_value_as_absent() {
        assert_eq!(
            resolve_key_from(Some(String::new()), Some("ring".to_string())),
            Some("ring".to_string())
        );
        assert_eq!(resolve_key_from(Some(String::new()), None), None);
    }

    /// The env stub is supplied explicitly so an ambient `OPENAI_API_KEY` cannot decide the
    /// outcome; the process env is not read.
    #[test]
    fn resolve_key_reads_a_stored_keyring_key() {
        let unset = |_: &str| None;
        let store = MemStore::new();
        store.set("openai", "sk-ring").unwrap();
        assert_eq!(
            resolve_key_with("openai", &store, unset),
            Some("sk-ring".to_string())
        );
        assert_eq!(resolve_key_with("openai", &MemStore::new(), unset), None);
    }

    #[test]
    fn resolve_key_reads_the_env_var_the_provider_declares() {
        let store = MemStore::new();
        store.set("openai", "sk-ring").unwrap();
        let only_openai = |var: &str| (var == "OPENAI_API_KEY").then(|| "sk-env".to_string());
        assert_eq!(
            resolve_key_with("openai", &store, only_openai),
            Some("sk-env".to_string())
        );
        // A provider with no declared env var never consults the environment.
        let store = MemStore::new();
        store.set("ollama", "sk-ring").unwrap();
        assert_eq!(
            resolve_key_with("ollama", &store, |_| Some("sk-env".to_string())),
            Some("sk-ring".to_string())
        );
    }

    #[test]
    fn apply_preferences_maps_preferences_and_keyring_keys() {
        let store = MemStore::new();
        store.set("openai", "sk-o").unwrap();
        let base = Selection::default();
        let selection = apply_preferences(base, &settings(Some("openai")), &store);
        assert_eq!(selection.preferred.as_deref(), Some("openai"));
        assert_eq!(selection.keys.get("openai"), Some(&"sk-o".to_string()));
    }

    #[test]
    fn apply_preferences_does_not_overwrite_an_env_key() {
        let store = MemStore::new();
        store.set("openai", "sk-ring").unwrap();
        let mut base = Selection::default();
        base.keys.insert("openai".to_string(), "sk-env".to_string());
        let selection = apply_preferences(base, &settings(None), &store);
        assert_eq!(selection.keys.get("openai"), Some(&"sk-env".to_string()));
    }

    #[test]
    fn build_and_info_with_nothing_configured_is_offline_local() {
        let (provider, info) = build_and_info(&Selection::default());
        assert_eq!(provider.id(), "local");
        assert_eq!(
            info.offline,
            Some(light_factory_providers::OfflineReason::NothingConfigured)
        );
        assert_eq!(info.selected_by, None);
    }

    #[test]
    fn build_and_info_with_a_stored_key_selects_that_provider() {
        let mut base = Selection {
            preferred: Some("deepseek".to_string()),
            ..Default::default()
        };
        base.keys.insert("deepseek".to_string(), "sk-d".to_string());
        let (provider, info) = build_and_info(&base);
        assert_eq!(provider.id(), "deepseek");
        assert_eq!(info.selected_by, Some(SelectedBy::StoredPreference));
    }

    #[test]
    fn build_and_info_uses_key_precedence_without_a_preference() {
        let mut base = Selection::default();
        base.keys
            .insert("anthropic".to_string(), "sk-a".to_string());
        let (provider, info) = build_and_info(&base);
        assert_eq!(provider.id(), "anthropic");
        assert_eq!(info.selected_by, Some(SelectedBy::KeyPrecedence));
    }
}
