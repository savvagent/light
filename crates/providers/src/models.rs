//! Model listing for the connect flow: free functions that fetch the model ids a provider
//! offers, decoupled from the pinned [`crate::Provider`] so they can run for a provider that is
//! not active — and with a freshly typed key. Each wrapper resolves the base URL through the
//! same trust boundary as provider construction ([`crate::selection::resolve_base_url_for`],
//! which routes `*_BASE_URL` overrides through `validate_base_url`) and every request uses the
//! redirect-disabled client (`build_http_client` + `reject_redirect`), so a 3xx or an invalid
//! override is refused before the API key is sent anywhere.

use serde::Deserialize;

use crate::base_url::{build_http_client, join_url, reject_redirect};

/// List model ids for a keyed provider against an already-resolved, already-validated `base_url`.
///
/// Deliberately does **not** re-validate `base_url`: the wrapper [`list_models`] is the trust
/// boundary and hands down a base that has already passed `validate_base_url`. Ids are
/// stable-sorted and deduped.
pub(crate) async fn list_models_at(
    provider: &str,
    base_url: &str,
    key: &str,
) -> anyhow::Result<Vec<String>> {
    match provider {
        "anthropic" => list_anthropic(base_url, key).await,
        "openai" => list_openai_compatible(base_url, "/v1/models", key).await,
        "gemini" => list_gemini(base_url, key).await,
        "deepseek" => list_openai_compatible(base_url, "/models", key).await,
        other => {
            anyhow::bail!("unknown provider '{other}'; expected anthropic|openai|gemini|deepseek")
        }
    }
}

/// Resolve `provider`'s base URL (env override → production default) and list its model ids.
pub async fn list_models(provider: &str, key: &str) -> anyhow::Result<Vec<String>> {
    let base = resolve_models_base(provider)?;
    list_models_at(provider, &base, key).await
}

/// List model ids from the local Ollama server at `base_url`, extracting each `name` (including
/// any `:tag`) verbatim so a tagged model can be selected.
pub(crate) async fn list_ollama_models_at(base_url: &str) -> anyhow::Result<Vec<String>> {
    let url = join_url(base_url, "/api/tags");
    let client = build_http_client(base_url);
    let raw = client.get(&url).send().await?;
    reject_redirect(&raw)?;
    let resp = raw.error_for_status()?.json::<OllamaTags>().await?;
    Ok(normalize(resp.models.into_iter().map(|m| m.name).collect()))
}

/// List model ids from the local Ollama server at the default localhost root.
pub async fn list_ollama_models() -> anyhow::Result<Vec<String>> {
    list_ollama_models_at(crate::ollama::LOCAL_BASE).await
}

/// Read the `*_BASE_URL` override for `provider` and resolve its base URL via the shared trust
/// boundary. This is the single place the env is read for model listing; the pure resolution
/// lives in [`crate::selection::resolve_base_url_for`].
fn resolve_models_base(provider: &str) -> anyhow::Result<String> {
    let override_value = crate::selection::RemoteChoice::parse(provider)
        .and_then(crate::selection::base_url_var)
        .and_then(std::env::var_os)
        .and_then(|raw| raw.into_string().ok())
        .filter(|v| !v.is_empty());
    crate::selection::resolve_base_url_for(provider, override_value)
}

async fn list_anthropic(base: &str, key: &str) -> anyhow::Result<Vec<String>> {
    let url = join_url(base, "/v1/models");
    let client = build_http_client(base);
    let raw = client
        .get(&url)
        .header("x-api-key", key)
        .header("anthropic-version", crate::anthropic::ANTHROPIC_VERSION)
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp = raw.error_for_status()?.json::<IdList>().await?;
    Ok(normalize(resp.data.into_iter().map(|m| m.id).collect()))
}

async fn list_openai_compatible(base: &str, path: &str, key: &str) -> anyhow::Result<Vec<String>> {
    let url = join_url(base, path);
    let client = build_http_client(base);
    let raw = client
        .get(&url)
        .header("authorization", format!("Bearer {key}"))
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp = raw.error_for_status()?.json::<IdList>().await?;
    Ok(normalize(resp.data.into_iter().map(|m| m.id).collect()))
}

async fn list_gemini(base: &str, key: &str) -> anyhow::Result<Vec<String>> {
    let url = join_url(base, "/v1beta/models");
    let client = build_http_client(base);
    let raw = client
        .get(&url)
        .header("x-goog-api-key", key)
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp = raw.error_for_status()?.json::<GeminiModels>().await?;
    let ids = resp
        .models
        .into_iter()
        .map(|m| {
            m.name
                .strip_prefix("models/")
                .map(str::to_string)
                .unwrap_or(m.name)
        })
        .collect();
    Ok(normalize(ids))
}

/// Stable-sort and dedup a model-id list.
fn normalize(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

#[derive(Deserialize)]
struct IdItem {
    id: String,
}

#[derive(Deserialize)]
struct IdList {
    #[serde(default)]
    data: Vec<IdItem>,
}

#[derive(Deserialize)]
struct GeminiModel {
    name: String,
}

#[derive(Deserialize)]
struct GeminiModels {
    #[serde(default)]
    models: Vec<GeminiModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn anthropic_lists_models_with_required_version_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "claude-sonnet-4" }, { "id": "claude-haiku-4-5" }]
            })))
            .mount(&server)
            .await;

        let ids = list_models_at("anthropic", &server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(ids, vec!["claude-haiku-4-5", "claude-sonnet-4"]);
    }

    #[tokio::test]
    async fn openai_lists_models_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "gpt-4o" }, { "id": "gpt-4o-mini" }]
            })))
            .mount(&server)
            .await;

        let ids = list_models_at("openai", &server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
    }

    #[tokio::test]
    async fn gemini_lists_models_and_strips_the_models_prefix() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .and(header("x-goog-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    { "name": "models/gemini-2.5-pro" },
                    { "name": "models/gemini-2.5-flash" }
                ]
            })))
            .mount(&server)
            .await;

        let ids = list_models_at("gemini", &server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(ids, vec!["gemini-2.5-flash", "gemini-2.5-pro"]);
    }

    #[tokio::test]
    async fn deepseek_lists_models_on_the_models_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "deepseek-chat" }]
            })))
            .mount(&server)
            .await;

        let ids = list_models_at("deepseek", &server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(ids, vec!["deepseek-chat"]);
    }

    #[tokio::test]
    async fn model_lists_are_deduped_and_stable_sorted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "zebra" }, { "id": "alpha" }, { "id": "alpha" }]
            })))
            .mount(&server)
            .await;

        let ids = list_models_at("openai", &server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(ids, vec!["alpha", "zebra"]);
    }

    #[tokio::test]
    async fn ollama_lists_tags_extracting_names_with_tags() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    { "name": "llama3.2:latest" },
                    { "name": "llama3.2" }
                ]
            })))
            .mount(&server)
            .await;

        let ids = list_ollama_models_at(&server.uri()).await.unwrap();
        assert_eq!(ids, vec!["llama3.2", "llama3.2:latest"]);
    }

    #[tokio::test]
    async fn an_auth_error_is_surfaced_as_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = list_models_at("openai", &server.uri(), "bad-key")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("401") || err.to_string().contains("status"));
    }

    #[tokio::test]
    async fn a_redirect_is_rejected_and_the_key_is_not_forwarded() {
        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"data":[{"id":"leaked"}]}"#)
                    .insert_header("content-type", "application/json"),
            )
            .mount(&upstream)
            .await;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/v1/models", upstream.uri()).as_str())
                    .insert_header("content-type", "application/json")
                    .set_body_string(r#"{"data":[{"id":"leaked"}]}"#),
            )
            .mount(&server)
            .await;

        let result = list_models_at("openai", &server.uri(), "test-key").await;
        assert!(
            result.is_err(),
            "a 3xx must be an error, not a parsed model list; got {result:?}"
        );
        assert!(
            upstream
                .received_requests()
                .await
                .expect("wiremock request recording must be enabled for this assertion")
                .is_empty(),
            "the redirect target must never receive the key"
        );
    }

    #[tokio::test]
    async fn an_unknown_provider_is_rejected_before_any_request() {
        assert!(
            list_models_at("local", "http://127.0.0.1:1", "k")
                .await
                .is_err()
        );
    }
}
