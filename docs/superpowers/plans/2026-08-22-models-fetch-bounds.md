# Bounded, Time-Limited Model-List Fetch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Stop a hostile, compromised, or merely unresponsive model-list endpoint from exhausting the
TUI's memory, hanging its modal forever, or keeping a credential-bearing connection open after the
user pressed Esc. Three bounds land in `crates/providers` (a per-request total deadline, a streaming
response-body byte cap, and a model-list length cap); one lands in `crates/tui` (the spawned fetch
task is stored and `abort()`ed on close, supersede, and session loss).

**Architecture:** A `pub(crate) ListBounds { timeout, max_body_bytes, max_models }` value threaded
through the existing `list_models_at` / `list_ollama_models_at` seams in
`crates/providers/src/models.rs`, so the public `list_models` / `list_ollama_models` wrappers keep
their signatures and pass `ListBounds::DEFAULT` while tests pin tight bounds. The deadline is a
per-request `RequestBuilder::timeout()` — a *total* deadline spanning DNS + connect + TLS + TTFB +
body read — so `build_http_client` is not touched and the completion path keeps its deliberate
absence of a deadline. `.json::<T>()` is replaced by a frame-by-frame capped read plus an explicit
`serde_json::from_slice`. In the TUI, two `Option<JoinHandle<()>>` fields follow the pattern
`leave_engine` / `engine_forward_task` already established.

**Tech Stack:** Rust (edition 2024, toolchain pinned by `rust-toolchain.toml`); existing `reqwest`
0.13 (`Response::chunk` and `RequestBuilder::timeout` are both in the crate's existing
`default-features = false, features = ["rustls", "json"]` build), existing `anyhow`/`serde_json`,
existing `wiremock` dev-dependency, existing `tokio` in the TUI. **No new dependency, no new
feature, no new crate.**

**Spec:** `docs/superpowers/specs/2026-08-22-models-fetch-bounds-design.md` — read it first. This
plan implements it exactly, including its §2 premise corrections (no separate `connect_timeout` and
no second HTTP client; truncation in `providers`, not in `fetch_model_list`; `close_connect` and
`dismiss_modals` leak identically to `close_models`; no `Content-Length` pre-check).

## Global Constraints

- **No AI attribution of any kind** — no `Co-Authored-By`, no "Generated with", no `🤖`, no AI credit
  in commit messages, code comments, docs, or the PR body. No comments unless the surrounding code
  already carries explanatory comments (this repo's providers/TUI modules do; match their density
  and their "why, not what" style).
- **Inward dependency flow untouched.** `providers` gains no dependency edge; `tui` remains a client
  leaf. `protocol` / `auth` / `persistence` / `server` / `web/` are not touched at all.
- **Secrets never logged or echoed.** No new error message may carry response-body content, a URL, a
  base URL, or an API key. The provider key travels only in the request headers it already travels
  in. **Do not use `serde_json::Error`'s `Display` in the parse error.** An earlier revision of this
  plan claimed it "reports a line/column only, never body content"; that is false for serde's
  `invalid_type` errors, which embed the offending value — a body of `{"data":"<attacker text>"}`
  renders as `invalid type: string "<attacker text>", expected a sequence`, and that message is
  interpolated into `connect.fetch_error` and drawn in the modal. Build the message from
  `Error::line()` and `Error::column()` by hand. Reaching it needs no credential: the Ollama path
  targets `127.0.0.1:11434`. See the spec's §11.1.
- **All three bounds reject; none degrades.** The byte cap, the deadline, and the list-length cap
  each produce an `Err`. Do **not** `truncate` an over-long list: the modal falls back to row 0 when
  the configured model is absent, so dropping it off the tail is a silent model substitution onto an
  attacker-chosen prefix. See the spec's Assumption 5 and §11.2.
- **The deadline is structural, not conventional.** Every model-list request is built through one
  `model_list_request` chokepoint, the way the byte cap is funnelled through `parse_capped`. Do not
  hand-repeat `.timeout(...)` on per-provider builder chains — that is the exact hazard
  `base_url.rs`'s module doc names.
- **The trust boundary is untouched.** `validate_base_url`, `build_http_client`, and
  `reject_redirect` keep their current behavior and ordering; `reject_redirect` still runs before any
  body is read and `error_for_status()` still runs before the capped read.
- **Semver: no bump.** Every new item is `pub(crate)`; `list_models` / `list_ollama_models` keep
  their exact signatures. Do **not** touch `Cargo.toml` `version` (Non-Negotiable Rule 6).
- **Keep the `crates/tui/src/app.rs` footprint minimal.** Two struct fields, two initializers, two
  small helpers, five one-line call sites, and the new tests — nothing else. Sibling PRs are in
  flight against `handle_models_fetched` / the `ModelsStep` error path (#47) and against extracting
  the modal machinery out of `app.rs` (#46). Do not restructure modals, do not touch
  `handle_connect_models` / `handle_models_fetched`, do not reformat unrelated regions.
- Run `cargo fmt --all` before every Rust commit. Lint gate:
  `cargo clippy --workspace --all-targets -- -D warnings`. Test gate: `cargo test --workspace`.
- Tests live next to the code they cover (`#[cfg(test)] mod tests`), and must be **offline and
  deterministic**: `wiremock` for HTTP, no live network, no keyring, no terminal, no assertion on
  elapsed wall-clock time.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `crates/providers/src/models.rs` | `ListBounds` + `DEFAULT`; `bounds` parameter on `list_models_at` / `list_ollama_models_at` and the four private helpers; `.timeout(bounds.timeout)` on each request; `read_capped` replacing `.json::<T>()`; `normalize(ids, max)` truncation; new + updated tests |
| Modify. `crates/providers/src/base_url.rs` | **Tests only** — pin that `build_http_client` puts no deadline on the client and none on a request built from it |
| Modify. `crates/tui/src/app.rs` | `connect_fetch_task` / `models_fetch_task` fields + initializers; `abort_connect_fetch` / `abort_models_fetch`; abort+store in `begin_fetch` / `begin_models_fetch`; abort in `close_connect` / `close_models` / `dismiss_modals`; clear on delivery in both handlers; `guard_panic` around the fetch; `dismiss_modals()` before the logout await; tests |
| Modify. `crates/tui/src/i18n.rs` | `connect.fetch_panicked` in EN and ES |
| Modify. `docs/superpowers/specs/2026-08-22-models-fetch-bounds-design.md` | Status → IMPLEMENTED at close-out |
| Modify. `docs/superpowers/plans/2026-08-22-models-fetch-bounds.md` | Checkboxes marked at close-out |

## Task Order & Rationale

Two tasks, in dependency order.

**Task 1 (providers) first** because it holds every behavioral bound and is self-contained: the
`*_at` seams are `pub(crate)`, the public wrappers do not change, and nothing in `crates/tui` needs
to compile differently. It must land before Task 2 so that a bisect between the two commits still
has a workspace that builds and a TUI whose fetch is already bounded.

**Task 2 (TUI) second** because it is pure lifecycle plumbing with no dependency on Task 1's types,
and keeping it in its own commit makes the `app.rs` diff — the one that must stay small for the
sibling PRs #46/#47 — reviewable in isolation.

A third split (deadline / byte cap / truncation as separate commits) was rejected: all three thread
through the same new `ListBounds` parameter, so splitting them would leave `ListBounds` fields unused
between commits, which `clippy -D warnings` rejects as dead code.

### Task 1: Bound the model-list fetch in `crates/providers`

**Files:** `crates/providers/src/models.rs`, `crates/providers/src/base_url.rs`

**Interfaces:**
- *Consumes:* `reqwest::RequestBuilder::timeout`, `reqwest::Response::chunk`, `serde_json::from_slice`,
  `std::time::Duration`. All available today; no manifest change. (`anyhow::Context` is **not**
  used — the parse error is built by hand from `Error::line()`/`Error::column()`.)
- *Produces:* `pub(crate) models::ListBounds` + `ListBounds::DEFAULT`; a `bounds: ListBounds`
  parameter on `pub(crate) list_models_at` / `pub(crate) list_ollama_models_at`; a private
  `read_capped`. **No public API change** — `pub async fn list_models(provider, key)` and
  `pub async fn list_ollama_models()` keep their exact signatures and pass `ListBounds::DEFAULT`.

- [ ] Add the failing tests to `crates/providers/src/models.rs`'s `#[cfg(test)] mod tests` (reusing
      the existing `wiremock::{Mock, MockServer, ResponseTemplate}` + `matchers::{header, method, path}`
      harness already imported there):
      - `an_oversized_body_is_refused_instead_of_buffered`: mount `/v1/models` returning a JSON body
        comfortably larger than a tight cap (e.g. 4096 ids), call
        `list_models_at("openai", &server.uri(), "k", ListBounds { max_body_bytes: 1_048_576, ..tight })`,
        assert `is_err()`, and assert against the **full** anyhow chain (`format!("{err:#}")`, not
        `err.to_string()`, which shows only the outermost message): it contains `"1048576"` and
        `"cap"`, and it contains **no** model id from the body — no response content may reach an
        error the modal renders.
        **The body must be ~3.7 MiB (150_000 ids, `model-{i:06}`) and the cap 1 MiB.** Both numbers
        are load-bearing: hyper's read buffer grows adaptively (a measured sequence for this body
        was 8081, 16384, 32768, 24697, …), so a cap below the *largest* frame makes the read bail on
        iteration one with an **empty** buffer, and the guard's cumulative behaviour is never
        exercised at all. Such a test passes just as happily against a per-frame cap
        (`chunk.len() > max_bytes`), which is functionally unbounded memory. Raising the cap merely
        above the *first* frame (e.g. 20_000) does not fix this.
      - `a_body_one_byte_over_the_cap_is_refused`: the same literal `BODY` with
        `max_body_bytes: BODY.len() - 1`, asserting `Err`. `a_body_at_the_cap_is_still_accepted`
        pins `cap`; without this one, relaxing the guard to `> max_bytes + 1` goes unnoticed.
      - `a_hostile_body_cannot_reach_the_modal_through_the_parse_error`: respond with
        `{"data":"<MARKER>"}` and assert the full `format!("{err:#}")` chain does not contain
        `<MARKER>` — same shape as the oversized-body assertion. This is what pins Global
        Constraints' parse-error rule.
      - `a_body_at_the_cap_is_still_accepted`: respond with `set_body_string(BODY)` where `BODY` is a
        JSON **string literal** (so its byte length is knowable — `set_body_json` never hands the
        implementer the serialized bytes), set `max_body_bytes: BODY.len()`, and assert `Ok`. This
        pins the boundary as `>` rather than `>=`.
      - `an_over_long_model_list_is_refused`: mount 20 ids (`m00`..`m19`), call with
        `max_models: 5`, assert `Err` whose chain names the count and the cap and carries no id.
      - `a_list_exactly_at_the_cap_is_accepted`: 5 ids with `max_models: 5` is `Ok`.
      - `a_stalled_endpoint_fails_at_the_deadline`: mount a response with
        `ResponseTemplate::new(200).set_delay(Duration::from_secs(5))` (comfortably past the
        deadline, while keeping a *failing* run — one where the timeout was not applied — bounded at
        5 s rather than 30), call with
        `timeout: Duration::from_millis(150)`, assert `is_err()`, and identify the timeout
        **structurally**:
        `err.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_timeout)`.
        Assert **nothing** about elapsed wall-clock time. Drive **every** provider path —
        `anthropic`, `openai`, `gemini`, `deepseek`, and `ollama` — from one `Mock` matched on
        `method("GET")` alone. Covering only `openai` leaves three of four request sites with no
        deadline coverage at all; ollama matters most, since it needs no credential and anything on
        `127.0.0.1:11434` reaches it.
      - `default_bounds_are_the_production_values`: assert
        `ListBounds::DEFAULT.timeout == Duration::from_secs(15)`,
        `max_body_bytes == 2 * 1024 * 1024`, `max_models == 1_000` — so a later accidental widening
        is a failing test rather than a silent regression.
      - Update the **nine** existing `list_models_at` / `list_ollama_models_at` call sites in that
        module to pass `ListBounds::DEFAULT` (`anthropic_lists_models_with_required_version_header`,
        `openai_lists_models_with_bearer`, `gemini_lists_models_and_strips_the_models_prefix`,
        `deepseek_lists_models_on_the_models_path`, `model_lists_are_deduped_and_stable_sorted`,
        `ollama_lists_tags_extracting_names_with_tags`, `an_auth_error_is_surfaced_as_an_error`,
        `a_redirect_is_rejected_and_the_key_is_not_forwarded`,
        `an_unknown_provider_is_rejected_before_any_request`). These double as the proof that an
        ordinary response still round-trips through the capped read and `from_slice`.
- [ ] Add the failing test to `crates/providers/src/base_url.rs`'s `#[cfg(test)] mod tests`:
      - `the_completion_client_carries_no_total_deadline`: for both an `https` and a loopback `http`
        base, assert
        `!format!("{:?}", build_http_client(base)).to_lowercase().contains("totaltimeout")`
        (reqwest emits a `reqwest::config::TotalTimeout` field in `Client`'s `Debug` only when one is
        configured) **and** that
        `build_http_client(base).get(url).build().unwrap().timeout().is_none()`. The second
        assertion is the structural one and is what makes "the timeout is scoped to the model-list
        calls" checkable rather than merely claimed. Scope the Debug half to a **total** deadline:
        asserting the absence of every timeout would cement "completions are unbounded" as a
        deliberate invariant and make #54 harder to close, when `read_timeout` and `connect_timeout`
        are both legitimate future additions. Treat the Debug half as best-effort — `Client`'s
        `Debug` renders `connect_timeout` not at all (that field lives on `ClientBuilder`'s
        `Debug`).
- [ ] Run `cargo test -p light-factory-providers` — expect compile failures (`ListBounds` does not
      exist; the `*_at` functions take three/one arguments).
- [ ] Implement in `crates/providers/src/models.rs`:
      - `use std::time::Duration;` at the top (`anyhow` is already a
        dependency).
      - `#[derive(Debug, Clone, Copy)] pub(crate) struct ListBounds { pub(crate) timeout: Duration,
        pub(crate) max_body_bytes: usize, pub(crate) max_models: usize }` with
        `impl ListBounds { pub(crate) const DEFAULT: Self = Self { timeout: Duration::from_secs(15),
        max_body_bytes: 2 * 1024 * 1024, max_models: 1_000 }; }`. Document each field with *why* the
        value is what it is (interactive modal; ~50x the largest plausible honest response; no
        provider publishes near 1000 ids), per the spec's Assumptions 1–3.
      - Thread `bounds: ListBounds` through `list_models_at`, `list_ollama_models_at`, `list_anthropic`,
        `list_openai_compatible`, and `list_gemini`. `list_models` and `list_ollama_models` pass
        `ListBounds::DEFAULT` — **do not change their signatures**.
      - Add `fn model_list_request(base: &str, path: &str, bounds: ListBounds) ->
        reqwest::RequestBuilder` returning
        `build_http_client(base).get(join_url(base, path)).timeout(bounds.timeout)`, and build all
        five provider paths through it (callers add only their own auth headers). Comment *why* it
        is per-request and not on the client — the same `build_http_client` serves completions,
        where a long generation is legitimate — and *why* it is a chokepoint: nothing fails if a
        hand-repeated `.timeout(...)` is omitted on a new provider path.
      - Add `ListBounds::debug_check()` and call it on both `*_at` seams: `timeout` non-zero,
        `max_models >= 1`, `max_body_bytes` at or above the smallest well-formed body. `max_models:
        0` is the one bound whose violation makes a fetch *succeed* with a wrong (empty) answer.
      - Add `async fn read_capped(mut resp: reqwest::Response, max_bytes: usize) -> anyhow::Result<Vec<u8>>`
        (`mut` is required — `Response::chunk` takes `&mut self`) that starts from an **empty** `Vec` (never reserving from an attacker-supplied length), loops
        `while let Some(chunk) = resp.chunk().await?`, bails with
        `anyhow::bail!("model list response exceeded the {max_bytes}-byte cap; refusing to buffer it")`
        when `buf.len() + chunk.len() > max_bytes`, and otherwise `extend_from_slice`. Document that
        dropping the response on bail closes the connection rather than draining it, and that peak
        buffering is `max_bytes` plus one HTTP-layer frame whose size the sender does not choose.
      - Replace all four `raw.error_for_status()?.json::<T>().await?` with
        `let resp = raw.error_for_status()?;` →
        `let body = read_capped(resp, bounds.max_body_bytes).await?;` →
        ```rust
        serde_json::from_slice(&body).map_err(|e| {
            anyhow::anyhow!(
                "model list response was not valid JSON (line {}, column {})",
                e.line(),
                e.column()
            )
        })
        ```
        (**not** `.context(...)` over `serde_json::Error` — see Global Constraints.)
        Keep `reject_redirect(&raw)?` before `error_for_status()`, exactly as today.
      - Change `normalize(mut ids: Vec<String>)` to
        `normalize(mut ids: Vec<String>, max: usize) -> anyhow::Result<Vec<String>>`, which after
        `sort` + `dedup` **bails** when `ids.len() > max` with a message naming the count and the cap
        and **no id**. Update the four call sites to pass `bounds.max_models` and propagate the
        `Result`. Document why refusing beats truncating (spec Assumption 5).
- [ ] Run `cargo test -p light-factory-providers` — all tests green, including the nine updated
      pre-existing ones.
- [ ] Verify the "no public API change" claim rather than remembering it:
      `grep -rn "normalize(\|list_models_at\|list_ollama_models_at" crates/ --include="*.rs"` must
      show hits only inside `crates/providers/src/models.rs`.
- [ ] Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` — both
      clean. This is what makes the Task Order rationale's "a bisect between the two commits still
      builds" a verified statement rather than an assumption: `crates/tui` calls the public wrappers,
      whose signatures did not change.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "providers: bound and time-limit the model-list fetch"`.
      No attribution trailer of any kind.

### Task 2: Abort the in-flight fetch task in the TUI

**Files:** `crates/tui/src/app.rs`

**Interfaces:**
- *Consumes:* `tokio::task::JoinHandle` (already used by `engine_forward_task`) and, in tests,
  `JoinHandle::abort_handle()`.
- *Produces:* private `App` fields and methods only. **No public API change, no i18n change, no
  rendering change, no state-machine change.**

- [ ] Add the failing tests to `crates/tui/src/app.rs`'s `#[cfg(test)] mod tests`, using the existing
      `test_app()` helper and `#[tokio::test]` (already used by
      `models_command_opens_a_fetching_list_for_the_active_provider`). Each test spawns a task that
      never completes (`tokio::spawn(std::future::pending::<()>())`), keeps a probe via
      `JoinHandle::abort_handle()` **before** moving the handle into the field, performs the action,
      then yields in a bounded loop (`for _ in 0..32 { if probe.is_finished() { break; }
      tokio::task::yield_now().await; }`) before asserting — never a `sleep`, never a wall-clock
      assertion:
      - `closing_the_models_modal_aborts_the_in_flight_fetch`: set `app.models_fetch_task`, call
        `app.close_models()`, assert `probe.is_finished()` and `app.models_fetch_task.is_none()`.
      - `closing_the_connect_modal_aborts_the_in_flight_fetch`: same via `app.close_connect()`.
      - `dismissing_modals_aborts_both_in_flight_fetches`: set both fields **with both modals
        `None`**, call `app.dismiss_modals()`, assert both probes finished — this is what pins the
        abort to *task* state rather than *modal* state (spec §6.5).
      - `starting_a_models_fetch_aborts_the_previous_one`: set `app.models_fetch_task` to a pending
        task, call `app.begin_models_fetch("local".to_string())` (`local` resolves no key, so the
        replacement fetch fails offline without touching the network), assert the **first** probe
        finished and `app.models_fetch_task.is_some()`.
      - `starting_a_connect_fetch_aborts_the_previous_one`: the mirror, via
        `app.begin_fetch("local".to_string(), None)`. Required, not optional — the three abort tests
        above assign the field by hand, so they exercise `abort_connect_fetch` but never the
        *producer*: `begin_fetch` could revert to a bare `tokio::spawn` with the handle never
        stored and the whole TUI suite would stay green, while `close_connect` would have nothing to
        abort and the credential-bearing connection would leak exactly as before.
      - `a_delivered_models_result_stops_tracking_its_task` /
        `a_delivered_connect_result_stops_tracking_its_task` — the handler clears its own handle.
        `a_stale_result_leaves_the_current_fetch_tracked` — a superseded nonce must **not** clear it.
      - `a_panicking_fetch_reports_an_error_instead_of_spinning_forever` — `guard_panic` over a
        panicking future yields the `connect.fetch_panicked` string.
- [ ] Run `cargo test -p light-factory-tui` — expect compile failures (the fields do not exist).
- [ ] Implement in `crates/tui/src/app.rs`:
      - Add `connect_fetch_task: Option<tokio::task::JoinHandle<()>>` and
        `models_fetch_task: Option<tokio::task::JoinHandle<()>>` to `struct App`, beside the existing
        `engine_forward_task`, and `connect_fetch_task: None` / `models_fetch_task: None` to
        `App::new`'s initializer.
      - Add `fn abort_connect_fetch(&mut self)` and `fn abort_models_fetch(&mut self)`, each
        `if let Some(task) = self.<field>.take() { task.abort(); }`, mirroring `leave_engine`'s
        existing shape. Document *why*: the request — and the API key in its headers — must not
        outlive the modal that asked for it, and this complements rather than replaces the
        `connect_nonce` / `models_nonce` stale-result guards, which stay exactly as they are.
      - `begin_fetch`: call `self.abort_connect_fetch();` before spawning, and store the returned
        handle in `self.connect_fetch_task`.
      - `begin_models_fetch`: call `self.abort_models_fetch();` before spawning, and store the handle
        in `self.models_fetch_task`.
      - `close_connect`: add `self.abort_connect_fetch();`.
      - `handle_connect_key`: abort (and bump `connect_nonce`) when a transition leaves a
        `ConnectStep::ModelList { fetching: true }` for something other than another fetching list.
        Esc there steps *back* to the provider list rather than closing the modal, so
        `close_connect` never runs — the one Esc path the abort helpers miss. The models modal has
        no equivalent gap: every Esc from `ModelsStep` is a `Close`.
      - `close_models`: add `self.abort_models_fetch();`.
      - `dismiss_modals`: add `self.abort_connect_fetch();` and `self.abort_models_fetch();`
        **unconditionally**, outside the existing `if self.connect.is_some()` /
        `if self.models.is_some()` arms.
      - Wrap the fetch in `guard_panic` (`futures_util::FutureExt::catch_unwind` over
        `std::panic::AssertUnwindSafe`), reporting a new `connect.fetch_panicked` i18n key (EN + ES).
        Nothing polls the `JoinHandle`, so an unwind inside the fetch sends no `UiEvent` at all and
        leaves `fetching: true` until Esc; the 15 s deadline is inside reqwest, not around the task.
      - Clear `connect_fetch_task` / `models_fetch_task` in `handle_connect_models` /
        `handle_models_fetched`, **after** the nonce check (a stale result must not drop the live
        fetch's handle), and document both fields as cancellation handles rather than "a fetch is in
        flight" predicates — #47 and #46 are both likely to reach for them as an "already fetching"
        guard, where they are wrong in both directions.
      - Move `self.dismiss_modals()` in `sign_out` **above** `self.api.logout(...).await`. The TUI's
        `Api` client is a `reqwest::Client::new()` with no timeout, so a server that never answers
        logout would otherwise delay cancellation of the key-bearing fetch indefinitely — the exact
        window this change set out to close.
      - Change **nothing else**. `apply_and_close_connect` / `apply_and_close_models` already
        delegate to `close_connect` / `close_models` and need no edit. Do not touch
        `handle_connect_models`, `handle_models_fetched`, `ModelsStep`, `ConnectStep`, the rendering
        functions, or `fetch_model_list`.
- [ ] Run `cargo test -p light-factory-tui` — green.
- [ ] Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` — both
      clean. (The `crates/persistence` integration test skips without `DATABASE_URL`; that is the
      documented pre-existing behavior, not a regression.)
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "tui: abort the in-flight model fetch on modal close"`.
      No attribution trailer of any kind.

## Out-of-Band Surfaces (Phase 5 verification)

**None touched.** This change modifies two Rust source files plus tests and two docs files. It does
not touch `Dockerfile`, `fly.toml`, `web/`, `crates/persistence/migrations/`, or `.github/`. Phase 5
step 14 is therefore vacuously satisfied — state it explicitly rather than skipping it.

## Close-Out Obligations

- [ ] **File a follow-up issue** for `crates/tui/src/api.rs:33` — `reqwest::Client::new()` gives the
      auth/sign-in HTTP path no timeout, the same unbounded-client pattern this change fixes for the
      model-list path. Do **not** fix it here (different crate, different trust boundary; the
      workflow's "same bug pattern found elsewhere" rule requires a filed issue, not scope creep).
- [ ] Update the spec's `> **Status:**` to IMPLEMENTED and mark this plan's checkboxes, then
      `git mv` both into `docs/superpowers/archive/{specs,plans}/`, add a row to
      `docs/superpowers/archive/README.md`, and commit as
      `docs: record the model-list fetch bounds as shipped`.
