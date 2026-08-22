# Bounded, time-limited model-list fetch — design

> **Status:** DRAFT — a per-request deadline, a streaming byte cap, and a list-length cap on the
> model-list path, plus an abortable fetch task in the TUI.

> **Implements:** https://github.com/savvagent/light/issues/44

## 1. Brief

Quoted from issue #44 (`providers: bound and time-limit the model-list fetch`):

> `build_http_client` (`crates/providers/src/base_url.rs`) sets a redirect and proxy policy but
> **no `.timeout()` and no `.connect_timeout()`**, and `list_models`/`list_ollama_models` buffer the
> whole response body via `.json::<IdList>()` with no size bound.
>
> Consequences:
> - A slow or non-responding endpoint leaves the `/models` and `/connect` modals showing
>   "Fetching models…" indefinitely. The footer says "Esc: cancel", and Esc does close the modal —
>   but `close_models` only bumps the nonce, so the spawned task keeps running and holds its
>   connection with the API key in memory.
> - A hostile or compromised endpoint can return an arbitrarily large JSON array and OOM the TUI.
>   The Ollama path needs no credential at all — anything listening on `127.0.0.1:11434` can do this.
>
> Note a blanket client timeout is **not** the right fix: the same client serves completion
> requests, where a long generation is legitimate. Scope the timeout to the model-list calls.
>
> Suggested:
> - A request timeout on the model-list calls specifically.
> - A byte cap while reading the body instead of `.json()`.
> - Truncate the list to a sane maximum in `fetch_model_list` before it reaches the UI.
> - Store the `JoinHandle` in `begin_fetch`/`begin_models_fetch` and `abort()` it on close
>   (`leave_engine` already does this correctly).

## 2. Premise corrections

The issue's premises are accurate about the defect. Three of its *suggested remedies* do not
survive contact with the code, and one under-counts the affected call sites.

1. **A separate `.connect_timeout()` is unnecessary and would cost a second HTTP client.**
   `connect_timeout` is a `ClientBuilder`-only knob (`reqwest-0.13.4/src/async_impl/client.rs:1469`);
   there is no per-request equivalent. Applying it would require forking `build_http_client` into a
   model-list variant — precisely the duplication `base_url.rs`'s module doc says caused the route
   policy to silently regress before ("every time one of them was defined in a single provider's
   file, a sibling provider was left behind"). It is also redundant: `RequestBuilder::timeout()` is a
   **total** deadline. reqwest arms it in `execute_request` before the request is handed to hyper
   (`client.rs:2653`) and threads the same `Sleep` into the response body
   (`async_impl/response.rs:36-46`), so it spans DNS + TCP + TLS + time-to-first-byte + body read.
   A connect timeout is a strictly weaker bound on a strict prefix of that interval.
   **Decision:** one per-request total timeout on the model-list calls. `build_http_client` is not
   modified at all, so the completion path's route policy and its (correct) absence of a deadline are
   untouched by construction.

2. **Truncation belongs in `providers`, not in `fetch_model_list`.** The issue names
   `crates/tui/src/app.rs::fetch_model_list` as the truncation site, but that function is a thin
   key-resolution wrapper over `list_models`/`list_ollama_models`; putting the cap there would leave
   `light_factory_providers::list_models` unbounded for every other caller (including a future
   non-TUI one) and would split one bound across two crates. The cap lands in `models.rs::normalize`,
   beside the existing sort/dedup, and `fetch_model_list` receives an already-bounded list.

3. **`close_models` is not the only leaking teardown.** `close_connect` (`app.rs:777`) and
   `dismiss_modals` (`app.rs:783`, called when the session goes away) bump their nonce and drop the
   modal without touching the spawned task, exactly like `close_models` (`app.rs:965`). All three are
   fixed, and `begin_fetch`/`begin_models_fetch` additionally abort any predecessor task before
   spawning a replacement (re-entering a modal or retyping a key currently strands the old fetch).

4. **A `Content-Length` pre-check is deliberately omitted.** It is attacker-controlled, may be absent
   on a chunked response, and buys nothing the streaming cap does not already give: the read stops
   after at most `max_body_bytes` + one frame regardless. A second, header-driven rejection path would
   be untestable through wiremock (which computes the header from the body it was given) and could
   diverge from the streaming path. One path, one bound.

## 3. Assumptions

1. **The deadline is 15 seconds.** The model-list fetch backs an interactive modal that renders
   "Fetching models…" with no progress; a provider that has not answered in 15s has already failed
   the user. Real `/v1/models` responses land in well under a second. Rationale: long enough to
   absorb a cold TLS handshake on a slow link, short enough that "Esc: cancel" is not the only exit.
2. **The body cap is 2 MiB.** OpenAI's `/v1/models` is tens of kilobytes; Ollama's `/api/tags` is a
   few. 2 MiB is roughly fifty times the largest plausible honest response and is a bound the TUI can
   absorb per in-flight fetch without any risk to a terminal process.
3. **The list cap is 1000 ids.** No provider publishes anywhere near that many models, and the modal
   is a scrolling list a human reads. Applied *after* sort+dedup so the retained subset is
   deterministic (the first 1000 ids in lexicographic order) rather than dependent on server order.
4. **The bounds are compile-time constants, not configuration.** Adding an env key would be a new
   public interface for a hardening change and would give an attacker-influenced environment a way to
   widen the bound. A `pub(crate) ListBounds` value threaded through the existing `*_at` test seams
   gives the tests tight bounds without exposing a knob.
5. **Truncation is silent to the user.** Surfacing "list truncated" would require changing
   `pub async fn list_models`'s return type (semver-major, Non-Negotiable Rule 6) for a state only a
   hostile or broken endpoint can produce. The cap is a safety bound, not a feature. Noted as a
   follow-up in §8 rather than widened into this change.
6. **`abort()` complements the nonce guard, it does not replace it.** Cancellation lands at the
   task's next await point, so a task that has already posted its `UiEvent` still delivers it. The
   existing `connect_nonce`/`models_nonce` stale-result checks stay exactly as they are and remain
   the correctness mechanism; the abort is what stops the *connection* (and the API key held in its
   request headers) from outliving the modal.
7. **The scope stays inside issue #44.** Sibling PRs are in flight against
   `handle_models_fetched`/`ModelsStep` error classification (#47) and against extracting the modal
   machinery out of `app.rs` (#46). This change therefore touches the smallest possible surface in
   `app.rs` — two fields, their initializers, two three-line helpers, and five call sites — and puts
   every behavioral bound in `crates/providers`.

## 4. Goal & Success Criteria

Goal: a hostile, compromised, or merely unresponsive model-list endpoint — including an
unauthenticated process on `127.0.0.1:11434`, which needs no credential to be reached — can no
longer exhaust the TUI's memory, hang its modal indefinitely, or keep a credential-bearing
connection open after the user pressed Esc.

- [ ] A model-list request that receives no response within `MODEL_LIST_TIMEOUT` fails with a
      timeout error instead of hanging; the completion path is provably unaffected.
- [ ] A response body larger than `MAX_BODY_BYTES` is rejected during the read, so at most
      `MAX_BODY_BYTES` (plus one frame) is ever buffered — never the whole body.
- [ ] A model list longer than `MAX_MODELS` is truncated before it leaves `crates/providers`.
- [ ] Closing (or dismissing) either modal, or starting a replacement fetch, aborts the in-flight
      fetch task rather than stranding it.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, and
      `cargo fmt --all --check` are clean; every new test is offline and deterministic.
- [ ] No public interface changes: `list_models`/`list_ollama_models` keep their signatures, so no
      crate `version` bump is required (§7).

## 5. Scope

**In:**
- `crates/providers/src/models.rs`: a `pub(crate) ListBounds` (deadline, body cap, list cap) with a
  `DEFAULT` constant; a per-request `.timeout()` on all five model-list requests; a streaming,
  byte-capped body read replacing `.json::<T>()`; a list-length cap in `normalize`.
- `crates/providers/src/base_url.rs`: tests only — pinning that `build_http_client` puts no deadline
  on the client and no deadline on a request built from it.
- `crates/tui/src/app.rs`: two `Option<JoinHandle<()>>` fields, their initializers, two abort
  helpers, and the abort calls in `begin_fetch`, `begin_models_fetch`, `close_connect`,
  `close_models`, and `dismiss_modals`.

**Out:**
- Any change to `build_http_client`, `reject_redirect`, `validate_base_url`, or the completion path.
  The completion path deliberately has no deadline and keeps it.
- Any change to `handle_connect_models` / `handle_models_fetched` / their error classification, to
  `ModelsStep`/`ConnectStep`, to the modal rendering, or to the modal state machines (#46, #47).
- Surfacing truncation, a cancellation notice, or a retry affordance in the UI.
- Making any bound configurable via env or `config.json`.
- Bounding any other network path (the completion providers, the auth `Api` client, the WebSocket).
  If those want bounds, they are separate issues (§8).

## 6. Design

### 6.1 `ListBounds` (`crates/providers/src/models.rs`)

```rust
/// The bounds a model-list fetch runs under. Threaded through the `*_at` seams so tests can pin
/// tight values without exposing a runtime knob.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListBounds {
    /// Total deadline for one model-list request: DNS + connect + TLS + TTFB + body read.
    pub(crate) timeout: Duration,
    /// Hard ceiling on buffered response bytes.
    pub(crate) max_body_bytes: usize,
    /// Hard ceiling on returned model ids.
    pub(crate) max_models: usize,
}

impl ListBounds {
    pub(crate) const DEFAULT: Self = Self {
        timeout: Duration::from_secs(15),
        max_body_bytes: 2 * 1024 * 1024,
        max_models: 1_000,
    };
}
```

`list_models_at` and `list_ollama_models_at` (both already `pub(crate)` test seams) gain a
`bounds: ListBounds` parameter and thread it into the four private per-provider helpers. The public
`list_models` / `list_ollama_models` wrappers pass `ListBounds::DEFAULT` — their signatures are
unchanged (§7).

### 6.2 The deadline

Each of the five requests (`list_anthropic`, `list_openai_compatible` ×2 paths, `list_gemini`,
`list_ollama_models_at`) gains `.timeout(bounds.timeout)` on its `RequestBuilder`, between the
headers and `.send()`. Nothing else acquires a deadline: `build_http_client` is untouched, so the
completion providers keep exactly the client they have today.

Pinned by tests in `base_url.rs`: a client from `build_http_client` renders no timeout in its
`Debug` (reqwest only emits `reqwest::config::TotalTimeout` when one is configured), and a request
built from it reports `Request::timeout() == None`.

### 6.3 The byte cap

`.json::<T>()` is replaced by a capped read plus an explicit deserialize:

```rust
/// Read a response body, refusing to buffer more than `max_bytes`.
///
/// `Response::bytes()`/`json()` buffer to completion, so a hostile endpoint's declared or actual
/// length becomes the TUI's allocation. This reads frame by frame and bails the moment the total
/// would exceed the cap, so peak memory is bounded by `max_bytes` plus one frame — and dropping
/// the response then closes the connection rather than draining the rest.
///
/// Nothing about the body reaches the error message: the sender controls its content, and this
/// error is rendered into the modal.
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
```

`Response::chunk()` is available without reqwest's `stream` feature
(`reqwest-0.13.4/src/async_impl/response.rs:310`; `bytes_stream` at :351 is the gated one), so no
dependency or feature change is needed. `buf` starts empty and grows only by what has actually
arrived — no capacity is ever reserved from an attacker-supplied length.

Call sites become:

```rust
let resp = raw.error_for_status()?;
let body = read_capped(resp, bounds.max_body_bytes).await?;
let parsed: IdList = serde_json::from_slice(&body)
    .context("model list response was not valid JSON")?;
```

`error_for_status()` still runs first and still discards the error body, so a 4xx/5xx never reaches
the capped read.

### 6.4 The list cap

```rust
/// Stable-sort, dedup, and cap a model-id list.
///
/// The cap is applied after sorting so the retained subset is deterministic — the first `max`
/// ids lexicographically — rather than whatever order the endpoint chose to emit.
fn normalize(mut ids: Vec<String>, max: usize) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids.truncate(max);
    ids
}
```

All four call sites pass `bounds.max_models`.

### 6.5 Abortable fetch tasks (`crates/tui/src/app.rs`)

Two fields beside the existing `engine_forward_task`, following its established pattern:

```rust
connect_fetch_task: Option<tokio::task::JoinHandle<()>>,
models_fetch_task: Option<tokio::task::JoinHandle<()>>,
```

initialized to `None`, and two helpers:

```rust
/// Cancel the connect modal's in-flight model fetch, so the request — and the API key in its
/// headers — does not outlive the modal that asked for it. Complements the `connect_nonce`
/// guard, which still discards any result that slips through before cancellation lands.
fn abort_connect_fetch(&mut self) {
    if let Some(task) = self.connect_fetch_task.take() {
        task.abort();
    }
}

fn abort_models_fetch(&mut self) { /* same, for models_fetch_task */ }
```

Wiring (five sites, each one line):

| Site | Change |
|---|---|
| `begin_fetch` | `self.abort_connect_fetch();` before spawning; store the new handle |
| `begin_models_fetch` | `self.abort_models_fetch();` before spawning; store the new handle |
| `close_connect` | `self.abort_connect_fetch();` |
| `close_models` | `self.abort_models_fetch();` |
| `dismiss_modals` | `self.abort_connect_fetch();` / `self.abort_models_fetch();` inside the existing `if` arms |

`apply_and_close_connect` and `apply_and_close_models` already delegate to `close_connect` /
`close_models`, so they are covered without edits. Nothing else in `app.rs` is touched.

## 7. Semver

No public interface changes, so **no `Cargo.toml` version bump** (Non-Negotiable Rule 6):

| Item | Visibility | Change |
|---|---|---|
| `light_factory_providers::list_models` | `pub` | none — signature and behavior contract unchanged |
| `light_factory_providers::list_ollama_models` | `pub` | none |
| `models::list_models_at`, `models::list_ollama_models_at` | `pub(crate)` | gain a `bounds` parameter — internal |
| `models::ListBounds` | `pub(crate)` (new) | internal |
| `base_url::build_http_client` | `pub(crate)` | **not modified** |
| `crates/tui` | binary/lib leaf | private fields and methods only |

`crates/providers/Cargo.toml` gains no dependency and no feature: `Response::chunk` and
`RequestBuilder::timeout` are both in the crate's existing `default-features = false, features =
["rustls", "json"]` build. `serde_json` is already a dependency; `anyhow::Context` is already in use
elsewhere in the crate.

Behavioral compatibility of the unchanged public functions: a caller that previously received a
>1000-entry list now receives 1000, and a caller that previously hung now receives an error. Both are
the point of the change, and neither alters a type.

## 8. Error handling & edge cases

| Case | Behavior |
|---|---|
| Endpoint never responds | `reqwest` timeout error after `timeout`; surfaced through the existing `{:#}` anyhow chain into `connect.fetch_error` |
| Endpoint responds, then stalls mid-body | Same deadline covers the body read (the `Sleep` is threaded into the response body), so the read errors rather than hanging |
| Body exceeds the cap | `read_capped` bails after at most `max_bytes` + one frame; the `Response` is dropped, closing the connection instead of draining |
| Body is under the cap but not JSON | `serde_json::from_slice` error with context. `serde_json::Error`'s `Display` reports only a line/column, never body content, so a hostile body cannot inject text into the modal |
| Body is valid JSON with no `data`/`models` key | Unchanged: both structs are `#[serde(default)]`, so this yields an empty list, which the modal already treats as an honest "no models reported" |
| List exceeds the cap | Silently truncated to the first `max_models` ids in sorted order (Assumption 5) |
| 3xx | Unchanged: `reject_redirect` still fires before the body is touched |
| 4xx/5xx | Unchanged: `error_for_status()` still fires before the capped read |
| Esc during a fetch | Modal closes (unchanged) **and** the task is aborted; if the result was already posted, the nonce guard discards it (unchanged) |
| Session lost during a fetch | `dismiss_modals` aborts both tasks |
| Re-entering a modal / retyping a key | `begin_*` aborts the predecessor before spawning |
| Task aborted after `events.send` | The `UnboundedSender` send already happened; the nonce guard rejects it. No change in observable behavior |

**Security properties preserved or added.** No error message gains body content, a URL, or a key.
An unauthenticated process on `127.0.0.1:11434` — the Ollama path needs no credential — can no
longer drive TUI allocation past `max_body_bytes`. A credential-bearing request no longer outlives
its modal, bounding how long a provider API key sits in an open connection's headers. The trust
boundary itself (`validate_base_url` → `build_http_client` → `reject_redirect`) is untouched.

## 9. Testing

All tests offline and deterministic; the provider tests use the crate's existing `wiremock` harness.

**`crates/providers/src/models.rs`:**
1. `an_oversized_body_is_refused_instead_of_buffered` — a body far larger than a tight
   `max_body_bytes` yields an `Err` naming the cap, and the error carries no body content.
2. `a_body_at_the_cap_is_still_accepted` — the boundary is not off by one.
3. `an_over_long_model_list_is_truncated` — 20 ids with `max_models: 5` returns exactly the first 5
   in sorted order (proving the cap is applied after sorting, not to arrival order).
4. `a_stalled_endpoint_fails_at_the_deadline` — wiremock delays past a `timeout` of ~150 ms; the call
   returns an `Err` promptly and the error is a timeout. This is the behavioral proof that the
   deadline is applied to the model-list path.
5. `default_bounds_are_the_production_values` — pins `ListBounds::DEFAULT` (15 s / 2 MiB / 1000), so
   a later accidental widening is a failing test rather than a silent regression.
6. Existing model-list tests updated for the new `*_at` signature and kept green (they double as the
   proof that an ordinary response still round-trips through the capped read and `from_slice`).

**`crates/providers/src/base_url.rs`:**
7. `the_completion_client_carries_no_deadline` — `build_http_client` renders no timeout in its
   `Debug`, and `client.get(url).build().unwrap().timeout()` is `None`. This is the counterpart to
   test 4 and is what makes "scoped to the model-list calls" checkable rather than asserted.

**`crates/tui/src/app.rs`** (`#[tokio::test]`, using `JoinHandle::abort_handle()` as a probe):
8. `closing_the_models_modal_aborts_the_in_flight_fetch`
9. `closing_the_connect_modal_aborts_the_in_flight_fetch`
10. `dismissing_modals_aborts_both_in_flight_fetches`
11. `starting_a_models_fetch_aborts_the_previous_one`

## 10. Risks & open questions

1. **Test 7 reads reqwest's `Debug` output.** A future reqwest release could rename
   `reqwest::config::TotalTimeout`. Mitigated by matching case-insensitively on `"timeout"` and by
   pairing it with the structural `Request::timeout()` assertion, which is a stable public API.
2. **15 s could be short for a very slow link or a proxied gateway.** The failure mode is a visible,
   localized error in a modal the user can retry, not data loss. If real reports arrive, raising the
   constant is a one-line change; making it configurable is deliberately deferred (Assumption 4).
3. **Truncation is silent.** A user on a hypothetical >1000-model endpoint would not know the list
   was cut. Follow-up rather than scope creep (Assumption 5).
4. **Other network paths remain unbounded** — the completion providers (intentionally), the auth
   `Api` client, and the WebSocket. Out of scope here (§5); worth a separate issue for the auth
   client, whose requests are short by nature and have no legitimate reason to hang.
5. **`app.rs` conflict risk with #46/#47.** Minimized by touching only field declarations,
   initializers, and five one-line call sites, and by adding no logic to
   `handle_connect_models`/`handle_models_fetched`. Merge order is #45 → #44 → #47 → #46.
