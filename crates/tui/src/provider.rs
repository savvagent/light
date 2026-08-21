//! Display-only provider metadata: the active provider's id/model plus why it was selected and
//! any offline reason. Construction (composition) lives in `crate::selection`, not here.

use light_factory_providers::{OfflineReason, SelectedBy};

use crate::i18n::{self, Locale};

/// The active provider's id, model, selection reason, and offline status, for the connected
/// header and the `/provider` listing.
#[derive(Clone)]
pub struct ProviderInfo {
    pub id: String,
    pub model: Option<String>,
    /// `Some(reason)` when the offline `LocalProvider` was selected; `None` for a live provider.
    pub offline: Option<OfflineReason>,
    /// Which rule selected a live provider; `None` when offline.
    pub selected_by: Option<SelectedBy>,
    /// Human-readable selection warnings, for the engine pane to surface.
    pub warnings: Vec<String>,
}

impl ProviderInfo {
    /// Render as `id` or `id (model)`.
    pub fn display(&self) -> String {
        match &self.model {
            Some(model) => format!("{} ({model})", self.id),
            None => self.id.clone(),
        }
    }

    /// A short localized phrase explaining why this provider is active (e.g. "key precedence",
    /// "stored preference", "offline"), or an empty string when there is nothing to add.
    pub fn reason(&self, locale: Locale) -> String {
        if self.offline.is_some() {
            return i18n::t(locale, "provider.reason.offline").to_string();
        }
        match self.selected_by {
            Some(SelectedBy::OllamaEnv) => {
                i18n::t(locale, "provider.reason.ollama_env").to_string()
            }
            Some(SelectedBy::RemoteSelectorEnv) => {
                i18n::t(locale, "provider.reason.selector_env").to_string()
            }
            Some(SelectedBy::StoredPreference) => {
                i18n::t(locale, "provider.reason.stored").to_string()
            }
            Some(SelectedBy::KeyPrecedence) => {
                i18n::t(locale, "provider.reason.key_precedence").to_string()
            }
            None => String::new(),
        }
    }
}

/// Map an [`OfflineReason`] to a localized notice naming the variable(s) to set.
pub fn offline_notice(locale: Locale, reason: &OfflineReason) -> String {
    match reason {
        OfflineReason::NothingConfigured => i18n::t(locale, "provider.offline.nothing").to_string(),
        OfflineReason::NamedProviderMissingKey { selector, key } => i18n::t_with(
            locale,
            "provider.offline.missing_key",
            &[("selector", selector), ("key", key)],
        ),
        OfflineReason::BaseUrlRejected { var } => {
            i18n::t_with(locale, "provider.offline.base_url", &[("var", var)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(offline: Option<OfflineReason>, selected_by: Option<SelectedBy>) -> ProviderInfo {
        ProviderInfo {
            id: "openai".to_string(),
            model: Some("gpt-4o-mini".to_string()),
            offline,
            selected_by,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn offline_notice_covers_each_reason() {
        assert_eq!(
            offline_notice(Locale::En, &OfflineReason::NothingConfigured),
            "No provider configured — set ANTHROPIC_API_KEY (or another provider's key) or LIGHT_OLLAMA=1"
        );
        assert_eq!(
            offline_notice(
                Locale::En,
                &OfflineReason::NamedProviderMissingKey {
                    selector: "openai".into(),
                    key: "OPENAI_API_KEY".into(),
                }
            ),
            "Provider 'openai' selected but OPENAI_API_KEY is not set — falling back to offline"
        );
        assert_eq!(
            offline_notice(
                Locale::En,
                &OfflineReason::BaseUrlRejected {
                    var: "LIGHT_OPENAI_BASE_URL".into(),
                }
            ),
            "LIGHT_OPENAI_BASE_URL was rejected — falling back to offline"
        );
    }

    #[test]
    fn display_appends_the_model_when_present() {
        assert_eq!(info(None, None).display(), "openai (gpt-4o-mini)");
    }

    #[test]
    fn reason_is_empty_without_a_source() {
        assert_eq!(info(None, None).reason(Locale::En), "");
    }
}
