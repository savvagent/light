//! Env-driven provider selection: read `LIGHT_*`/`*_API_KEY` variables, choose a single
//! provider, and degrade to the offline `LocalProvider` when nothing is configured. The
//! decision helpers are pure (injectable inputs, no process env) so the precedence table is
//! unit-testable without `set_var`.

use crate::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, LocalProvider, OllamaProvider,
    OpenAiProvider, Provider, validate_base_url,
};

/// Default model ids when the corresponding `LIGHT_*_MODEL` env var is unset (otto constants).
const DEFAULT_OLLAMA_MODEL: &str = "llama3.2";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-haiku-4-5";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

/// A provider built from the environment, plus its effective model id (for display).
pub struct BuiltProvider {
    pub provider: Box<dyn Provider>,
    pub model: Option<String>,
}

/// Which remote provider the single remote slot uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteChoice {
    Anthropic,
    OpenAi,
    Gemini,
    DeepSeek,
}

impl RemoteChoice {
    /// Stable id matching each provider's `Provider::id()`.
    fn id(self) -> &'static str {
        match self {
            RemoteChoice::Anthropic => "anthropic",
            RemoteChoice::OpenAi => "openai",
            RemoteChoice::Gemini => "gemini",
            RemoteChoice::DeepSeek => "deepseek",
        }
    }
}

/// Which slot the environment selects: Ollama (local HTTP), a remote, or offline Local.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    Ollama,
    Remote(RemoteChoice),
    Local,
}

/// Pure precedence: `LIGHT_OLLAMA=1` wins over any remote; a remote wins over offline Local.
fn choose(ollama_on: bool, remote: Option<RemoteChoice>) -> Slot {
    if ollama_on {
        Slot::Ollama
    } else if let Some(choice) = remote {
        Slot::Remote(choice)
    } else {
        Slot::Local
    }
}

/// Pure remote-slot selection. `selector` is the raw `LIGHT_REMOTE_PROVIDER` value; the four
/// bools are "this provider's key is present and non-empty". A valid selector wins when its key
/// is present; a selector whose key is absent yields `None` (offline) rather than silently
/// falling back to another provider; an unknown selector is ignored and key precedence applies:
/// Anthropic > OpenAI > Gemini > DeepSeek.
fn select_remote_from(
    selector: Option<&str>,
    anthropic: bool,
    openai: bool,
    gemini: bool,
    deepseek: bool,
) -> Option<RemoteChoice> {
    if let Some(sel) = selector {
        match sel.to_ascii_lowercase().as_str() {
            "anthropic" => {
                return present_or_warn(anthropic, RemoteChoice::Anthropic, "ANTHROPIC_API_KEY");
            }
            "openai" => return present_or_warn(openai, RemoteChoice::OpenAi, "OPENAI_API_KEY"),
            "gemini" => return present_or_warn(gemini, RemoteChoice::Gemini, "GEMINI_API_KEY"),
            "deepseek" => {
                return present_or_warn(deepseek, RemoteChoice::DeepSeek, "DEEPSEEK_API_KEY");
            }
            other => {
                eprintln!(
                    "warning: LIGHT_REMOTE_PROVIDER='{other}' is not a known provider \
                     (anthropic|openai|gemini|deepseek); using key precedence instead"
                );
            }
        }
    }
    if anthropic {
        Some(RemoteChoice::Anthropic)
    } else if openai {
        Some(RemoteChoice::OpenAi)
    } else if gemini {
        Some(RemoteChoice::Gemini)
    } else if deepseek {
        Some(RemoteChoice::DeepSeek)
    } else {
        None
    }
}

/// Return the choice if its key is present, else warn and select nothing (offline) — a
/// named-but-unusable selector must not misroute to another key.
fn present_or_warn(present: bool, choice: RemoteChoice, key: &str) -> Option<RemoteChoice> {
    if present {
        Some(choice)
    } else {
        eprintln!(
            "warning: LIGHT_REMOTE_PROVIDER='{}' but {key} is not set; \
             falling back to the offline/local provider",
            choice.id()
        );
        None
    }
}

/// True when the given provider's API key is present and non-empty in the environment.
fn has_key(choice: RemoteChoice) -> bool {
    let var = match choice {
        RemoteChoice::Anthropic => "ANTHROPIC_API_KEY",
        RemoteChoice::OpenAi => "OPENAI_API_KEY",
        RemoteChoice::Gemini => "GEMINI_API_KEY",
        RemoteChoice::DeepSeek => "DEEPSEEK_API_KEY",
    };
    std::env::var(var).map(|k| !k.is_empty()).unwrap_or(false)
}

/// Env-reading wrapper over `select_remote_from` — the default remote selection.
fn select_remote() -> Option<RemoteChoice> {
    select_remote_from(
        std::env::var("LIGHT_REMOTE_PROVIDER").ok().as_deref(),
        has_key(RemoteChoice::Anthropic),
        has_key(RemoteChoice::OpenAi),
        has_key(RemoteChoice::Gemini),
        has_key(RemoteChoice::DeepSeek),
    )
}

/// The `LIGHT_<P>_MODEL` override variable for a provider.
fn model_var(choice: RemoteChoice) -> &'static str {
    match choice {
        RemoteChoice::Anthropic => "LIGHT_ANTHROPIC_MODEL",
        RemoteChoice::OpenAi => "LIGHT_OPENAI_MODEL",
        RemoteChoice::Gemini => "LIGHT_GEMINI_MODEL",
        RemoteChoice::DeepSeek => "LIGHT_DEEPSEEK_MODEL",
    }
}

/// The default model constant for a provider (used when its `LIGHT_<P>_MODEL` var is unset).
fn default_model_constant(choice: RemoteChoice) -> &'static str {
    match choice {
        RemoteChoice::Anthropic => DEFAULT_ANTHROPIC_MODEL,
        RemoteChoice::OpenAi => DEFAULT_OPENAI_MODEL,
        RemoteChoice::Gemini => DEFAULT_GEMINI_MODEL,
        RemoteChoice::DeepSeek => DEFAULT_DEEPSEEK_MODEL,
    }
}

/// The effective model for a provider: an explicit override, else the constant.
fn default_model_for(choice: RemoteChoice, model_override: Option<String>) -> String {
    model_override.unwrap_or_else(|| default_model_constant(choice).to_string())
}

/// Read a provider's `LIGHT_<P>_MODEL` override, treating an exported-but-empty value as unset.
fn env_model(choice: RemoteChoice) -> Option<String> {
    std::env::var(model_var(choice))
        .ok()
        .filter(|v| !v.is_empty())
}

/// Resolve a provider's base URL from an optional `*_BASE_URL` override.
///
/// `Some(url)` is the base to use (normalized). `None` means an override was supplied but
/// refused — the caller must construct no provider, so the API key is never sent anywhere.
fn resolve_base_url(
    override_value: Option<String>,
    default: &str,
    var_name: &str,
) -> Option<String> {
    let Some(value) = override_value.filter(|v| !v.is_empty()) else {
        return Some(default.to_string());
    };
    match validate_base_url(&value) {
        Ok(normalized) => Some(normalized),
        Err(e) => {
            eprintln!(
                "warning: {var_name} was rejected: {e}; falling back to the offline/local \
                 provider rather than sending the API key"
            );
            None
        }
    }
}

/// The `LIGHT_<P>_BASE_URL` override variable for a provider, or `None` when it has none.
fn base_url_var(choice: RemoteChoice) -> Option<&'static str> {
    match choice {
        RemoteChoice::OpenAi => Some("LIGHT_OPENAI_BASE_URL"),
        RemoteChoice::DeepSeek => Some("LIGHT_DEEPSEEK_BASE_URL"),
        // Fixed endpoints — `api_base_default()` only, no operator input reaches them.
        RemoteChoice::Anthropic | RemoteChoice::Gemini => None,
    }
}

/// Read and validate a provider's `LIGHT_<P>_BASE_URL` override, defaulting to the production
/// endpoint when unset, and yielding `None` (offline) on an invalid override.
fn env_base_url(choice: RemoteChoice, default: &str) -> Option<String> {
    let Some(var_name) = base_url_var(choice) else {
        return Some(default.to_string());
    };
    match std::env::var_os(var_name) {
        None => Some(default.to_string()),
        Some(raw) => match raw.into_string() {
            Ok(value) => resolve_base_url(Some(value), default, var_name),
            Err(_) => {
                eprintln!(
                    "warning: {var_name} is not valid UTF-8; falling back to the offline/local \
                     provider rather than sending the API key to the default endpoint"
                );
                None
            }
        },
    }
}

/// Construct the remote provider for `choice`, pinned to `model`. Returns `None` when the
/// provider's `*_BASE_URL` override fails validation.
fn build_remote(choice: RemoteChoice, model: String) -> Option<Box<dyn Provider>> {
    match choice {
        RemoteChoice::Anthropic => Some(Box::new(AnthropicProvider::new(
            AnthropicProvider::api_base_default(),
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model,
        ))),
        RemoteChoice::OpenAi => {
            let base = env_base_url(choice, OpenAiProvider::api_base_default())?;
            Some(Box::new(OpenAiProvider::new(
                base,
                std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                model,
            )))
        }
        RemoteChoice::Gemini => Some(Box::new(GeminiProvider::new(
            GeminiProvider::api_base_default(),
            std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            model,
        ))),
        RemoteChoice::DeepSeek => {
            let base = env_base_url(choice, DeepSeekProvider::api_base_default())?;
            Some(Box::new(DeepSeekProvider::new(
                base,
                std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
                model,
            )))
        }
    }
}

/// Build the single provider slot from the environment, always yielding a usable provider
/// (the offline `LocalProvider` as the fail-closed default — selection never errors).
pub fn build_provider_from_env() -> BuiltProvider {
    let ollama_on = std::env::var("LIGHT_OLLAMA").as_deref() == Ok("1");
    let remote = select_remote();

    match choose(ollama_on, remote) {
        Slot::Ollama => {
            let model = std::env::var("LIGHT_OLLAMA_MODEL")
                .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
            BuiltProvider {
                provider: Box::new(OllamaProvider::local_default(model.clone())),
                model: Some(model),
            }
        }
        Slot::Remote(choice) => {
            let model = default_model_for(choice, env_model(choice));
            match build_remote(choice, model.clone()) {
                Some(provider) => BuiltProvider {
                    provider,
                    model: Some(model),
                },
                None => BuiltProvider {
                    provider: Box::new(LocalProvider::new()),
                    model: None,
                },
            }
        }
        Slot::Local => BuiltProvider {
            provider: Box::new(LocalProvider::new()),
            model: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_wins_over_any_remote() {
        assert_eq!(choose(true, Some(RemoteChoice::Anthropic)), Slot::Ollama);
        assert_eq!(choose(true, None), Slot::Ollama);
    }

    #[test]
    fn remote_wins_over_local() {
        assert_eq!(
            choose(false, Some(RemoteChoice::OpenAi)),
            Slot::Remote(RemoteChoice::OpenAi)
        );
        assert_eq!(choose(false, None), Slot::Local);
    }

    #[test]
    fn a_valid_selector_with_its_key_present_selects_that_provider() {
        assert_eq!(
            select_remote_from(Some("gemini"), false, false, true, false),
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(Some("ANTHROPIC"), true, false, false, false),
            Some(RemoteChoice::Anthropic)
        );
    }

    #[test]
    fn a_valid_selector_whose_key_is_absent_selects_nothing() {
        // Named "openai" but no OPENAI_API_KEY: must NOT fall through to the (present)
        // Anthropic key. Silent misrouting is what this guard exists to prevent.
        assert_eq!(
            select_remote_from(Some("openai"), true, false, false, false),
            None
        );
    }

    #[test]
    fn an_unknown_selector_falls_through_to_key_precedence() {
        assert_eq!(
            select_remote_from(Some("bogus"), true, false, false, false),
            Some(RemoteChoice::Anthropic)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, true, false, false),
            Some(RemoteChoice::OpenAi)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, false, true, false),
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, false, false, true),
            Some(RemoteChoice::DeepSeek)
        );
    }

    #[test]
    fn key_precedence_is_anthropic_then_openai_then_gemini_then_deepseek() {
        assert_eq!(
            select_remote_from(None, true, true, true, true),
            Some(RemoteChoice::Anthropic)
        );
        assert_eq!(
            select_remote_from(None, false, true, true, true),
            Some(RemoteChoice::OpenAi)
        );
        assert_eq!(
            select_remote_from(None, false, false, true, true),
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(None, false, false, false, true),
            Some(RemoteChoice::DeepSeek)
        );
        assert_eq!(select_remote_from(None, false, false, false, false), None);
    }

    #[test]
    fn resolve_base_url_accepts_a_valid_override_normalized() {
        // The `url` crate strips surrounding whitespace and lowercases the scheme, and appends a
        // trailing slash for an empty path — so the returned value is the NORMALIZED form, not the
        // operator's raw string.
        assert_eq!(
            resolve_base_url(
                Some(" HTTPS://api.openai.com ".to_string()),
                "https://api.openai.com",
                "LIGHT_OPENAI_BASE_URL"
            ),
            Some("https://api.openai.com/".to_string())
        );
    }

    #[test]
    fn resolve_base_url_rejects_a_bad_override() {
        assert_eq!(
            resolve_base_url(
                Some("http://evil.example.com".to_string()),
                "https://api.openai.com",
                "LIGHT_OPENAI_BASE_URL"
            ),
            None
        );
    }

    #[test]
    fn resolve_base_url_uses_the_default_when_unset_or_empty() {
        assert_eq!(
            resolve_base_url(None, "https://default", "LIGHT_OPENAI_BASE_URL"),
            Some("https://default".to_string())
        );
        assert_eq!(
            resolve_base_url(
                Some(String::new()),
                "https://default",
                "LIGHT_OPENAI_BASE_URL"
            ),
            Some("https://default".to_string())
        );
    }

    #[test]
    fn default_models_match_otto_constants() {
        assert_eq!(
            default_model_constant(RemoteChoice::Anthropic),
            "claude-haiku-4-5"
        );
        assert_eq!(default_model_constant(RemoteChoice::OpenAi), "gpt-4o-mini");
        assert_eq!(
            default_model_constant(RemoteChoice::Gemini),
            "gemini-2.5-flash"
        );
        assert_eq!(
            default_model_constant(RemoteChoice::DeepSeek),
            "deepseek-v4-flash"
        );
    }

    #[test]
    fn model_override_wins_over_the_constant() {
        assert_eq!(
            default_model_for(RemoteChoice::OpenAi, Some("gpt-5".to_string())),
            "gpt-5"
        );
        assert_eq!(default_model_for(RemoteChoice::OpenAi, None), "gpt-4o-mini");
    }
}
