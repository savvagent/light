//! Build the active LLM provider from the environment and expose a small info record for
//! display. Construction is fail-closed: selection never errors (it always yields at least the
//! offline `LocalProvider`).

use std::sync::Arc;

use light_factory_providers::{Provider, build_provider_from_env};

/// The active provider's id and model, for the connected header.
pub struct ProviderInfo {
    pub id: String,
    pub model: Option<String>,
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
    };
    (Arc::from(built.provider), info)
}
