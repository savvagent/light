//! light-factory provider implementations (in-process libraries behind
//! `light_factory_engine_core::Provider`).

pub mod anthropic;
pub mod base_url;
pub mod deepseek;
pub mod gemini;
pub mod local;
pub mod ollama;
pub mod openai;
mod openai_compatible;
pub mod scripted;
pub mod selection;

pub use anthropic::AnthropicProvider;
pub use base_url::{BaseUrlError, validate_base_url};
pub use deepseek::DeepSeekProvider;
pub use gemini::GeminiProvider;
pub use local::LocalProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use scripted::ScriptedProvider;
pub use selection::{
    BuiltProvider, OfflineReason, SelectedBy, Selection, build_provider, build_provider_from_env,
    env_key_var,
};

// Re-export the seam from engine-core for consumer convenience.
pub use light_factory_engine_core::{CompleteRequest, CompleteResponse, Provider, Usage};
