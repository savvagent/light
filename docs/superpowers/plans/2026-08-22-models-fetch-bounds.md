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
  in. `serde_json::Error`'s `Display` reports a line/column only, never body content — do not add a
  body snippet to the parse-error context.
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
| Modify. `crates/tui/src/app.rs` | `connect_fetch_task` / `models_fetch_task` fields + initializers; `abort_connect_fetch` / `abort_models_fetch`; abort+store in `begin_fetch` / `begin_models_fetch`; abort in `close_connect` / `close_models` / `dismiss_modals`; four `#[tokio::test]`s |
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
  `anyhow::Context`, `std::time::Duration`. All available today; no manifest change.
- *Produces:* `pub(crate) models::ListBounds` + `ListBounds::DEFAULT`; a `bounds: ListBounds`
  parameter on `pub(crate) list_models_at` / `pub(crate) list_ollama_models_at`; a private
  `read_capped`. **No public API change** — `pub async fn list_models(provider, key)` and
  `pub async fn list_ollama_models()` keep their exact signatures and pass `ListBounds::DEFAULT`.

- [ ] Add the failing tests to `crates/providers/src/models.rs`'s `#[cfg(test)] mod tests` (reusing
      the existing `wiremock::{Mock, MockServer, ResponseTemplate}` + `matchers::{header, method, path}`
      harness already imported there):
      - `an_oversized_body_is_refused_instead_of_buffered`: mount `/v1/models` returning a JSON body
        comfortably larger than a tight cap (e.g. 4096 ids), call
        `list_models_at("openai", &server.uri(), "k", ListBounds { max_body_bytes: 512, ..tight })`,
        assert `is_err()`, and assert against the **full** anyhow chain (`format!("{err:#}")`, not
        `err.to_string()`, which shows only the outermost message): it contains `"512"` and `"cap"`,
        and it contains **no** model id from the body — no response content may reach an error the
        modal renders.
      - `a_body_at_the_cap_is_still_accepted`: respond with `set_body_string(BODY)` where `BODY` is a
        JSON **string literal** (so its byte length is knowable — `set_body_json` never hands the
        implementer the serialized bytes), set `max_body_bytes: BODY.len()`, and assert `Ok`. This
        pins the boundary as `>` rather than `>=`.
      - `an_over_long_model_list_is_truncated`: mount 20 ids named so that sorted order is
        unambiguous (e.g. `m00`..`m19`), call with `max_models: 5`, assert the result is exactly
        `["m00","m01","m02","m03","m04"]` — proving the cap is applied *after* the sort, not to
        arrival order.
      - `a_stalled_endpoint_fails_at_the_deadline`: mount a response with
        `ResponseTemplate::new(200).set_delay(Duration::from_secs(5))` (comfortably past the
        deadline, while keeping a *failing* run — one where the timeout was not applied — bounded at
        5 s rather than 30), call with
        `timeout: Duration::from_millis(150)`, assert `is_err()`, and identify the timeout
        **structurally**:
        `err.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_timeout)`.
        Assert **nothing** about elapsed wall-clock time.
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
      - `the_completion_client_carries_no_deadline`: for both an `https` and a loopback `http` base,
        assert `!format!("{:?}", build_http_client(base)).to_lowercase().contains("timeout")`
        (reqwest emits a `reqwest::config::TotalTimeout` field in `Client`'s `Debug` only when one is
        configured) **and** that
        `build_http_client(base).get(url).build().unwrap().timeout().is_none()`. The second
        assertion is the structural one and is what makes "the timeout is scoped to the model-list
        calls" checkable rather than merely claimed.
- [ ] Run `cargo test -p light-factory-providers` — expect compile failures (`ListBounds` does not
      exist; the `*_at` functions take three/one arguments).
- [ ] Implement in `crates/providers/src/models.rs`:
      - `use std::time::Duration;` and `use anyhow::Context;` at the top (`anyhow` is already a
        dependency; the crate does not currently import `Context` anywhere).
      - `#[derive(Debug, Clone, Copy)] pub(crate) struct ListBounds { pub(crate) timeout: Duration,
        pub(crate) max_body_bytes: usize, pub(crate) max_models: usize }` with
        `impl ListBounds { pub(crate) const DEFAULT: Self = Self { timeout: Duration::from_secs(15),
        max_body_bytes: 2 * 1024 * 1024, max_models: 1_000 }; }`. Document each field with *why* the
        value is what it is (interactive modal; ~50x the largest plausible honest response; no
        provider publishes near 1000 ids), per the spec's Assumptions 1–3.
      - Thread `bounds: ListBounds` through `list_models_at`, `list_ollama_models_at`, `list_anthropic`,
        `list_openai_compatible`, and `list_gemini`. `list_models` and `list_ollama_models` pass
        `ListBounds::DEFAULT` — **do not change their signatures**.
      - Add `.timeout(bounds.timeout)` to each of the four `RequestBuilder` chains, between the
        headers and `.send()`. Comment *why* it is per-request and not on the client: the same
        `build_http_client` serves completions, where a long generation is legitimate.
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
        `let resp: T = serde_json::from_slice(&body).context("model list response was not valid JSON")?;`
        Keep `reject_redirect(&raw)?` before `error_for_status()`, exactly as today.
      - Change `normalize(mut ids: Vec<String>)` to `normalize(mut ids: Vec<String>, max: usize)`,
        adding `ids.truncate(max)` **after** `sort` + `dedup`, and update the four call sites to pass
        `bounds.max_models`. Document that truncating after the sort is what makes the retained
        subset deterministic.
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
      - `close_models`: add `self.abort_models_fetch();`.
      - `dismiss_modals`: add `self.abort_connect_fetch();` and `self.abort_models_fetch();`
        **unconditionally**, outside the existing `if self.connect.is_some()` /
        `if self.models.is_some()` arms.
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
