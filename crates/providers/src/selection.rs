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

/// Why selection fell back to the offline `LocalProvider`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineReason {
    /// No provider configured: no `LIGHT_OLLAMA`, no `LIGHT_REMOTE_PROVIDER`, and none of the
    /// API keys are present.
    NothingConfigured,
    /// `LIGHT_REMOTE_PROVIDER` named a provider whose API key is absent.
    NamedProviderMissingKey { selector: String, key: String },
    /// A `*_BASE_URL` override was rejected (invalid or non-UTF-8); no provider was constructed
    /// so the API key is never sent to an unvalidated host.
    BaseUrlRejected { var: String },
}

/// A provider built from the environment, plus its effective model id (for display).
pub struct BuiltProvider {
    pub provider: Box<dyn Provider>,
    pub model: Option<String>,
    /// `Some(reason)` when the offline `LocalProvider` was selected; `None` for a live provider
    /// (Ollama or a remote).
    pub offline: Option<OfflineReason>,
    /// Human-readable selection warnings, for the TUI to surface (formerly `eprintln!`).
    pub warnings: Vec<String>,
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

/// The remote-slot selection: which remote (if any), plus why it is `None` and any warnings.
struct RemoteSelection {
    choice: Option<RemoteChoice>,
    offline: Option<OfflineReason>,
    warnings: Vec<String>,
}

/// A `*_BASE_URL` override that was refused, carrying the variable name and the human-readable
/// reason so the caller can surface both.
#[derive(Debug)]
struct BaseUrlRejection {
    var: String,
    warning: String,
}

/// A constructed remote provider, or the reason it fell back to offline plus warnings.
struct RemoteBuild {
    provider: Option<Box<dyn Provider>>,
    offline: Option<OfflineReason>,
    warnings: Vec<String>,
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
) -> RemoteSelection {
    let mut warnings = Vec::new();
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
                warnings.push(format!(
                    "warning: LIGHT_REMOTE_PROVIDER='{other}' is not a known provider \
                     (anthropic|openai|gemini|deepseek); using key precedence instead"
                ));
            }
        }
    }
    let choice = if anthropic {
        Some(RemoteChoice::Anthropic)
    } else if openai {
        Some(RemoteChoice::OpenAi)
    } else if gemini {
        Some(RemoteChoice::Gemini)
    } else if deepseek {
        Some(RemoteChoice::DeepSeek)
    } else {
        None
    };
    RemoteSelection {
        choice,
        offline: choice.is_none().then_some(OfflineReason::NothingConfigured),
        warnings,
    }
}

/// Return the choice if its key is present, else warn and select nothing (offline) — a
/// named-but-unusable selector must not misroute to another key.
fn present_or_warn(present: bool, choice: RemoteChoice, key: &str) -> RemoteSelection {
    if present {
        RemoteSelection {
            choice: Some(choice),
            offline: None,
            warnings: Vec::new(),
        }
    } else {
        RemoteSelection {
            choice: None,
            offline: Some(OfflineReason::NamedProviderMissingKey {
                selector: choice.id().to_string(),
                key: key.to_string(),
            }),
            warnings: vec![format!(
                "warning: LIGHT_REMOTE_PROVIDER='{}' but {key} is not set; \
                 falling back to the offline/local provider",
                choice.id()
            )],
        }
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
fn select_remote() -> RemoteSelection {
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
/// `Ok(url)` is the base to use (normalized). `Err(rejection)` means an override was supplied
/// but refused — the caller must construct no provider, so the API key is never sent anywhere.
fn resolve_base_url(
    override_value: Option<String>,
    default: &str,
    var_name: &str,
) -> Result<String, BaseUrlRejection> {
    let Some(value) = override_value.filter(|v| !v.is_empty()) else {
        return Ok(default.to_string());
    };
    match validate_base_url(&value) {
        Ok(normalized) => Ok(normalized),
        Err(e) => Err(BaseUrlRejection {
            var: var_name.to_string(),
            warning: format!(
                "warning: {var_name} was rejected: {e}; falling back to the offline/local \
                 provider rather than sending the API key"
            ),
        }),
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
/// endpoint when unset, and yielding `Err(rejection)` (offline) on an invalid override.
fn env_base_url(choice: RemoteChoice, default: &str) -> Result<String, BaseUrlRejection> {
    let Some(var_name) = base_url_var(choice) else {
        return Ok(default.to_string());
    };
    match std::env::var_os(var_name) {
        None => Ok(default.to_string()),
        Some(raw) => match raw.into_string() {
            Ok(value) => resolve_base_url(Some(value), default, var_name),
            Err(_) => Err(BaseUrlRejection {
                var: var_name.to_string(),
                warning: format!(
                    "warning: {var_name} is not valid UTF-8; falling back to the offline/local \
                     provider rather than sending the API key to the default endpoint"
                ),
            }),
        },
    }
}

/// Construct the remote provider for `choice`, pinned to `model`. Returns `None` when the
/// provider's `*_BASE_URL` override fails validation.
fn build_remote(choice: RemoteChoice, model: String) -> RemoteBuild {
    match choice {
        RemoteChoice::Anthropic => RemoteBuild {
            provider: Some(Box::new(AnthropicProvider::new(
                AnthropicProvider::api_base_default(),
                std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
                model,
            ))),
            offline: None,
            warnings: Vec::new(),
        },
        RemoteChoice::OpenAi => {
            let base = env_base_url(choice, OpenAiProvider::api_base_default());
            remote_with_base_url(base, "OPENAI_API_KEY", model, |base, key, model| {
                Box::new(OpenAiProvider::new(base, key, model))
            })
        }
        RemoteChoice::Gemini => RemoteBuild {
            provider: Some(Box::new(GeminiProvider::new(
                GeminiProvider::api_base_default(),
                std::env::var("GEMINI_API_KEY").unwrap_or_default(),
                model,
            ))),
            offline: None,
            warnings: Vec::new(),
        },
        RemoteChoice::DeepSeek => {
            let base = env_base_url(choice, DeepSeekProvider::api_base_default());
            remote_with_base_url(base, "DEEPSEEK_API_KEY", model, |base, key, model| {
                Box::new(DeepSeekProvider::new(base, key, model))
            })
        }
    }
}

/// Turn a resolved (or rejected) base URL into a [`RemoteBuild`], so the base-URL rejection path
/// is shared by every provider that has a `*_BASE_URL` override.
fn remote_with_base_url(
    base: Result<String, BaseUrlRejection>,
    key: &str,
    model: String,
    build: impl FnOnce(String, String, String) -> Box<dyn Provider>,
) -> RemoteBuild {
    match base {
        Ok(base) => RemoteBuild {
            provider: Some(build(base, std::env::var(key).unwrap_or_default(), model)),
            offline: None,
            warnings: Vec::new(),
        },
        Err(rejection) => RemoteBuild {
            provider: None,
            offline: Some(OfflineReason::BaseUrlRejected { var: rejection.var }),
            warnings: vec![rejection.warning],
        },
    }
}

/// Build the single provider slot from the environment, always yielding a usable provider
/// (the offline `LocalProvider` as the fail-closed default — selection never errors).
pub fn build_provider_from_env() -> BuiltProvider {
    let ollama_on = std::env::var("LIGHT_OLLAMA").as_deref() == Ok("1");
    let remote = select_remote();
    let mut warnings = remote.warnings;
    let remote_offline = remote.offline;

    match choose(ollama_on, remote.choice) {
        Slot::Ollama => {
            let model = std::env::var("LIGHT_OLLAMA_MODEL")
                .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
            BuiltProvider {
                provider: Box::new(OllamaProvider::local_default(model.clone())),
                model: Some(model),
                offline: None,
                warnings,
            }
        }
        Slot::Remote(choice) => {
            let model = default_model_for(choice, env_model(choice));
            let built = build_remote(choice, model.clone());
            warnings.extend(built.warnings);
            match built.provider {
                Some(provider) => BuiltProvider {
                    provider,
                    model: Some(model),
                    offline: None,
                    warnings,
                },
                None => BuiltProvider {
                    provider: Box::new(LocalProvider::new()),
                    model: None,
                    offline: built.offline,
                    warnings,
                },
            }
        }
        Slot::Local => BuiltProvider {
            provider: Box::new(LocalProvider::new()),
            model: None,
            offline: remote_offline,
            warnings,
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
            select_remote_from(Some("gemini"), false, false, true, false).choice,
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(Some("ANTHROPIC"), true, false, false, false).choice,
            Some(RemoteChoice::Anthropic)
        );
    }

    #[test]
    fn a_valid_selector_whose_key_is_absent_selects_nothing() {
        // Named "openai" but no OPENAI_API_KEY: must NOT fall through to the (present)
        // Anthropic key. Silent misrouting is what this guard exists to prevent.
        let selection = select_remote_from(Some("openai"), true, false, false, false);
        assert_eq!(selection.choice, None);
        assert_eq!(
            selection.offline,
            Some(OfflineReason::NamedProviderMissingKey {
                selector: "openai".to_string(),
                key: "OPENAI_API_KEY".to_string(),
            })
        );
        assert_eq!(selection.warnings.len(), 1);
        assert!(selection.warnings[0].contains("OPENAI_API_KEY is not set"));
    }

    #[test]
    fn an_unknown_selector_falls_through_to_key_precedence() {
        assert_eq!(
            select_remote_from(Some("bogus"), true, false, false, false).choice,
            Some(RemoteChoice::Anthropic)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, true, false, false).choice,
            Some(RemoteChoice::OpenAi)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, false, true, false).choice,
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(Some("bogus"), false, false, false, true).choice,
            Some(RemoteChoice::DeepSeek)
        );
    }

    #[test]
    fn an_unknown_selector_warns_but_keeps_the_live_provider() {
        let selection = select_remote_from(Some("bogus"), true, false, false, false);
        assert_eq!(selection.choice, Some(RemoteChoice::Anthropic));
        assert_eq!(selection.offline, None);
        assert_eq!(selection.warnings.len(), 1);
        assert!(selection.warnings[0].contains("not a known provider"));
    }

    #[test]
    fn nothing_configured_reports_the_offline_reason() {
        let selection = select_remote_from(None, false, false, false, false);
        assert_eq!(selection.choice, None);
        assert_eq!(selection.offline, Some(OfflineReason::NothingConfigured));
        assert!(selection.warnings.is_empty());
    }

    #[test]
    fn key_precedence_is_anthropic_then_openai_then_gemini_then_deepseek() {
        assert_eq!(
            select_remote_from(None, true, true, true, true).choice,
            Some(RemoteChoice::Anthropic)
        );
        assert_eq!(
            select_remote_from(None, false, true, true, true).choice,
            Some(RemoteChoice::OpenAi)
        );
        assert_eq!(
            select_remote_from(None, false, false, true, true).choice,
            Some(RemoteChoice::Gemini)
        );
        assert_eq!(
            select_remote_from(None, false, false, false, true).choice,
            Some(RemoteChoice::DeepSeek)
        );
        assert_eq!(
            select_remote_from(None, false, false, false, false).choice,
            None
        );
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
            )
            .unwrap(),
            "https://api.openai.com/"
        );
    }

    #[test]
    fn resolve_base_url_rejects_a_bad_override() {
        let rejection = resolve_base_url(
            Some("http://evil.example.com".to_string()),
            "https://api.openai.com",
            "LIGHT_OPENAI_BASE_URL",
        )
        .unwrap_err();
        assert_eq!(rejection.var, "LIGHT_OPENAI_BASE_URL");
        assert!(
            rejection
                .warning
                .contains("LIGHT_OPENAI_BASE_URL was rejected")
        );
    }

    #[test]
    fn resolve_base_url_uses_the_default_when_unset_or_empty() {
        assert_eq!(
            resolve_base_url(None, "https://default", "LIGHT_OPENAI_BASE_URL").unwrap(),
            "https://default"
        );
        assert_eq!(
            resolve_base_url(
                Some(String::new()),
                "https://default",
                "LIGHT_OPENAI_BASE_URL"
            )
            .unwrap(),
            "https://default"
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
