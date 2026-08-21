//! Build the active LLM provider from the environment and expose a small info record for
//! display. Construction is fail-closed: selection never errors (it always yields at least the
//! offline `LocalProvider`).

use std::sync::Arc;

use light_factory_providers::{OfflineReason, Provider, build_provider_from_env};

use crate::i18n::{self, Locale};

/// The active provider's id and model, for the connected header.
pub struct ProviderInfo {
    pub id: String,
    pub model: Option<String>,
    /// `Some(reason)` when the offline `LocalProvider` was selected; `None` for a live provider.
    pub offline: Option<OfflineReason>,
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
}

/// Build the provider and its info record from the environment.
pub fn build() -> (Arc<dyn Provider>, ProviderInfo) {
    let built = build_provider_from_env();
    let id = built.provider.id().to_string();
    let info = ProviderInfo {
        id,
        model: built.model,
        offline: built.offline,
        warnings: built.warnings,
    };
    (Arc::from(built.provider), info)
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
}
