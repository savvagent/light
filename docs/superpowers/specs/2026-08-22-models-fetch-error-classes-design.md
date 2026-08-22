# Model-list fetch error classes — design

> **Status:** DRAFT — branch the `/models` modal on *why* the fetch failed: credential failures point at `/connect`/`/key`, transport failures offer a retry plus an explicitly-unverified manual entry.

> **Implements:** https://github.com/savvagent/light/issues/47
> **Follows:** https://github.com/savvagent/light/issues/36 (the `/models` modal this corrects)

## 1. Brief

`handle_models_fetched` (`crates/tui/src/app.rs:900`) routes **every** error class into
`ModelsStep::Manual`, whose prompt reads "Type a model id, or Esc to close"
(`i18n.rs` `models.manual`). Three defects follow:

1. **The remedy is wrong for credential failures.** For a 401, a 403, or "No API key for openai",
   typing a model id cannot fix anything — the user needs `/connect` or `/key`. The modal
   nonetheless accepts the id, persists it, and reports "Model set to o3", so the user believes the
   problem is solved and hits the same auth failure on their next `/ask`.
2. **No retry.** `models_step_next`'s `Manual` arm (`app.rs:2419`) has no key that re-triggers the
   fetch, so a one-second blip permanently degrades the modal to manual entry until it is closed and
   reopened.
3. **Manual entries are reported as if verified.** `persist_model` (`app.rs:660`) sets
   `status.model_set` — "Model set to {model}" — identically whether the id came from the provider's
   own list or was typed blind. A typo surfaces much later as an opaque 404.

The issue also records *why* the "no API key" case is reachable at all despite `enter_models`'
offline guard (`app.rs:868`): `provider_info.offline` is a snapshot from the last
`rebuild_provider()`, while `resolve_key` re-reads the keyring live, so a keyring that locks
mid-session produces it while the header still shows the provider as connected. That is confirmed
by reading `crate::selection::rebuild` / `resolve_key` — the guard is a stale snapshot, not a
live check, so the credential branch is load-bearing rather than defensive.

## 2. Scope

**In:**
- A classification of model-list fetch failures into `MissingKey` / `Auth` / `Fetch`, computed at
  the TUI's `fetch_model_list` boundary (`app.rs:2355`).
- A new `ModelsStep::Credentials` terminal step for `MissingKey`/`Auth`, rendering the failure and
  the remedy (`/connect`, `/key set <provider> <key>`) with **no** input box.
- A retry key (`Ctrl+R`) on the transport-failure step, re-running the fetch in place.
- Relabelling the manual box as an unverified fallback, and a distinct
  `status.model_set_unverified` status string for ids that were typed rather than picked.
- EN + ES catalog entries for every new string.

**Out:**
- **#44's fetch bounds** (timeout, byte cap, list truncation, stored `JoinHandle`/`abort()`). This
  design only guarantees that a timeout or oversize failure lands in the `Fetch` bucket, which it
  does by construction (`Fetch` is the default class).
- **#46's modal extraction.** `ModelsStep`/`ModelsState` are not renamed or moved; the help and
  connect modals are untouched.
- The `/connect` modal's own fetch-error rendering. `begin_fetch` keeps its
  `Result<Vec<String>, String>` event payload by mapping the new error to its message, so
  `ConnectStep::ModelList`'s error path is byte-for-byte unchanged.
- A typed error enum inside `crates/providers` (see Assumption 1 and Follow-ups).
- Any change to `/model <id>`'s parsing or persistence path beyond the status string it reports.

## 3. Premise corrections

1. **This narrows AC 6 of #36 on purpose.** #36 AC 6 reads "When the model list cannot be fetched
   (network error, **bad key**), the user can fall back to typing a model id manually." #47 is a
   deliberate correction of the "bad key" half: a blind id cannot repair a rejected credential, and
   offering it is the silent failure #47 was filed for. AC 6 remains satisfied in full for the
   network-blip case (manual entry still offered, now with a retry key on top) and is intentionally
   superseded for the credential case. `/model <id>` itself is untouched, so a user who genuinely
   wants to set an id blind while a key is broken still can.
2. **"Bad key" is not always distinguishable at the HTTP layer.** Anthropic, OpenAI and DeepSeek
   answer an invalid key with 401; Gemini answers some invalid keys with **400
   INVALID_ARGUMENT**. A 400 therefore classifies as `Fetch`, i.e. retry-plus-unverified-manual —
   exactly today's behaviour. The change is strictly an improvement over the status quo for the
   statuses it does recognise, and never a regression for the ones it does not (see Risks).
3. **The `Manual` step is only ever reached from a failed fetch.** It has no other constructor in
   `app.rs`, so relabelling it as the unverified fallback does not mislabel any other path.

## 4. Assumptions

1. **Classification happens in the TUI, by downcasting the `anyhow` chain to `reqwest::Error`.**
   `list_models`/`list_ollama_models` return `anyhow::Result<Vec<String>>`; the error produced by
   `error_for_status()?` is a `reqwest::Error` carrying the status. `fetch_model_list` walks
   `err.chain()` and takes the first `reqwest::Error`'s `.status()`.
   *Rationale:* (a) `crates/tui` already depends on `reqwest` directly (`api.rs`), so this adds no
   dependency edge and does not touch the inward dependency flow; (b) downcasting is the sanctioned
   `anyhow` idiom for an untyped error; (c) `crates/providers/src/models.rs` is being edited
   concurrently by #44, and a typed-error refactor there would be a public-API (semver) change plus
   a guaranteed merge conflict. Walking the whole `chain()` rather than downcasting the root keeps
   this correct if #44 adds `.context(...)`.
2. **Only 401 and 403 mean "credential".** Everything else — including 400, 404, 429 and 5xx —
   classifies as `Fetch`. *Rationale:* those are the two statuses that unambiguously mean
   "the credential you sent was refused"; 429 is a rate limit that a retry genuinely fixes, and
   guessing at 400 would misroute real bad-request bugs into a dead-end step with no retry.
3. **`MissingKey` is produced only where the TUI already knows it.** `fetch_model_list`'s
   `None => Err(connect.no_key)` arm is the sole `MissingKey` source; it is not inferred from a
   message. *Rationale:* string-sniffing a localized message is fragile and locale-dependent.
4. **`MissingKey` and `Auth` share one step but not one message.** Both land in
   `ModelsStep::Credentials`, because the remedy is identical. `MissingKey`'s message
   ("No API key for openai") is already a complete sentence naming the provider and is shown
   verbatim; `Auth` wraps the transport detail in `models.auth_rejected`
   ("{provider} rejected the credential: {error}"). *Rationale:* wrapping `MissingKey` would render
   "openai rejected the credential: No API key for openai".
5. **Retry is offered on the transport step only, not the credential step.** Re-running the fetch
   with the same still-broken credential would fail identically and teach the user nothing.
   *Rationale:* the credential step's whole point is to redirect to `/connect`/`/key`; Esc/Enter
   close it so the user can run those commands.
6. **The retry key is `Ctrl+R`.** The manual step consumes bare `Char(c)` as text, so an unmodified
   letter is unavailable. `Ctrl+R` is placed **before** the `Char(c)` arm in `models_step_next`;
   a plain `r` still types an `r`. *Rationale:* `Ctrl+R` is the conventional "reload"; `F5` is
   unreliable across terminal emulators.
7. **Retry reuses `begin_models_fetch`, which bumps `models_nonce`.** The retry transition resets
   the step to `ModelList { fetching: true, models: [] }` and re-fetches, so an in-flight prior
   result is discarded by the existing stale-nonce guard. *Rationale:* that guard is the existing,
   tested mechanism; retry needs no new machinery. This also composes cleanly with #44's
   `JoinHandle`/`abort()` — a nonce bump already makes the superseded result inert.
8. **`/model <id>` also reports the unverified status.** `set_model` (`app.rs:1031`) is by
   definition a blind, typed id, so it passes `verified: false`. *Rationale:* the alternative is
   `persist_model(..., verified: true)` at a call site that verifies nothing, which is the same lie
   #47 was filed about, one call site over. It is a one-argument change with no behavioural effect
   beyond the status wording.
9. **The connect modal keeps its `String` error payload.** `UiEvent::ConnectModels.result` stays
   `Result<Vec<String>, String>`; `begin_fetch` maps the new error type to `.message`.
   *Rationale:* #47 is scoped to the `/models` modal, and #46 is about to move the connect modal.
10. **`FetchError`/`FetchFailure` stay private to `crates/tui`.** No public API, no wire type, no
    `protocol` change, therefore no `Cargo.toml` version bump (Non-Negotiable Rule 6).

## 5. Design

### 5.1 Error classification (`crates/tui/src/app.rs`)

```rust
/// Why a model-list fetch failed, in the only terms the modal has to act on.
enum FetchFailure {
    /// No API key could be resolved for the provider at all.
    MissingKey,
    /// The provider refused the credential we sent (401/403).
    Auth,
    /// Anything else: DNS, refused connection, TLS, timeout, 5xx, malformed body.
    Fetch,
}

/// A failed model-list fetch: the class the modal branches on, plus the detail to render.
struct FetchError { class: FetchFailure, message: String }
```

`FetchFailure::needs_credentials()` returns `true` for `MissingKey | Auth` — the single predicate
the modal branches on, so a future class only has to answer that question.

`fetch_model_list` returns `Result<Vec<String>, FetchError>`:

| Source | Class |
|---|---|
| `resolve_key` returned `None` | `MissingKey`, message = existing `connect.no_key` string |
| `anyhow` chain contains a `reqwest::Error` whose `.status()` is 401 or 403 | `Auth` |
| everything else (incl. Ollama transport failures, timeouts, oversize bodies from #44) | `Fetch` |

`classify_fetch_error(&anyhow::Error) -> FetchFailure` is a free function; the status→class mapping
is separated into a pure `class_for_status(Option<u16>)` so it is testable without a socket.

### 5.2 The new step (`ModelsStep::Credentials`)

```rust
ModelsStep::Credentials { provider: String, error: String }
```

Rendered by `draw_models` as: the failure message (red), a blank line, then
`models.credentials_hint` (dark gray) — "Typing a model id can't fix this. Use /connect, or
/key set {provider} <key>". Footer reuses the existing `models.footer_offline` ("Esc: close").
No input box, no list, `focus: None`.

`models_step_next` for `Credentials`: `Esc | Enter → Close`, every other key → `Step(self.clone())`
(matching `Offline`, the other terminal notice step). `models_apply_target` returns `None`, so the
step can never persist anything.

A small `ModelsStep::provider(&self) -> Option<&str>` accessor is added so the retry path can read
the provider from any step without a three-arm match at the call site.

### 5.3 Retry

`ModelsTransition` gains `Retry`. `models_step_next`'s `Manual` arm maps
`Char('r') + CONTROL → Retry`, ordered before `Enter`/`Backspace`/`Char(c)`.
`handle_models_key` maps `Retry → self.retry_models_fetch()`, which resets the step to
`ModelList { provider, models: vec![], selected: 0, fetching: true }` and calls
`begin_models_fetch(provider)` (nonce bump included). The modal is thus fully recoverable from a
blip without closing and reopening.

### 5.4 Verified vs. unverified applies

`models_apply_target` returns `Option<ModelChoice>`:

```rust
struct ModelChoice { provider: String, model: String, verified: bool }
```

`verified: true` from `ModelList` (the id came off the provider's own list), `verified: false` from
`Manual`. `persist_model(provider, model, verified)` picks the status string:

- `verified` → `status.model_set` — "Model set to {model}" (unchanged)
- `!verified` → `status.model_set_unverified` — "Model set to {model} — not verified against {provider}"

Call sites: `apply_and_close_connect` → `true` (picked from a fetched list);
`apply_and_close_models` → the choice's flag; `set_model` (`/model <id>`) → `false`.

### 5.5 i18n (`crates/tui/src/i18n.rs`) — EN **and** ES, parity test-enforced

| Key | EN |
|---|---|
| `status.model_set_unverified` | `Model set to {model} — not verified against {provider}` |
| `models.auth_rejected` | `{provider} rejected the credential: {error}` |
| `models.credentials_hint` | `Typing a model id can't fix this. Use /connect, or /key set {provider} <key>` |
| `models.manual_unverified` | `Type a model id to use anyway — it won't be checked against {provider}` (replaces `models.manual`) |
| `models.footer_manual` | `Enter: save unverified · Ctrl+R: retry · Esc: close` (value updated) |

`models.manual` is removed from both catalogs — nothing references it once `draw_models` uses the
provider-aware replacement. The existing `es_mirrors_en_exactly` test in `i18n.rs` enforces parity
and is kept green.

## 6. Error handling & edge cases

| Case | Behaviour |
|---|---|
| Successful but **empty** list | Unchanged — stays a `ModelList` with the "no models reported" notice. An empty list is not a failure. |
| Late result for a superseded fetch | Unchanged — `models_nonce` guard drops it. Retry bumps the nonce, so a slow first response cannot overwrite the retry's state. |
| Late result while the user is typing in `Manual` | Unchanged — `handle_models_fetched` only accepts results while the step is `ModelList { fetching: true }`, so typed input is never clobbered. Retry deliberately leaves `Manual`, discarding the partial id; that is the user's explicit action. |
| Session lost mid-fetch | Unchanged — `dismiss_modals` clears the modal and bumps the nonce. `Credentials` needs no special handling. |
| Ollama (keyless) | Never `MissingKey` (no key is resolved); a refused connection to localhost is `Fetch`, so retry + unverified manual is offered. Correct: `ollama serve` may simply not be running yet. |
| `unknown provider '<x>'` bail from `list_models_at` | `Fetch`. A programming error surfaced as a retryable failure is the status quo; no new dead end. |
| Blank manual id + Enter | Unchanged — `models_apply_target` returns `None`, the modal stays open. |
| `Ctrl+C` in any step | Unchanged — intercepted by `handle_models_key` before the transition function. |

## 7. Testing

All in `crates/tui/src/app.rs`'s `#[cfg(test)] mod tests`, offline-deterministic, next to the code.

1. `models_step_next` on `Credentials`: `Esc → Close`, `Enter → Close`, a char → `Step` (unchanged).
2. `models_step_next` on `Manual`: `Ctrl+R → Retry`; a bare `r` still appends `r` to the input.
3. `handle_models_fetched` with `class: Auth` → `Credentials` naming the provider; with
   `class: MissingKey` → `Credentials` carrying the message verbatim; with `class: Fetch` →
   `Manual { input: "", error: Some(_) }`.
4. `handle_models_key(Ctrl+R)` from `Manual` re-triggers the fetch: step becomes
   `ModelList { fetching: true }` for the same provider and `models_nonce` advances (`#[tokio::test]`
   — `begin_models_fetch` spawns).
5. Manual apply reports the unverified status; list apply reports the verified one.
6. `models_apply_target` returns `verified: true` from a list and `verified: false` from manual, and
   `None` from `Credentials`.
7. `class_for_status`: `401`/`403` → `Auth`; `400`/`404`/`429`/`500`/`None` → `Fetch`.
8. `classify_fetch_error` against a real `reqwest` error: a wiremock 401 → `Auth`, a 500 → `Fetch`,
   a 401 wrapped in `.context(...)` → `Auth` (proves chain traversal), a connection-refused error →
   `Fetch`. Requires `wiremock` as a `crates/tui` **dev**-dependency (already in `Cargo.lock` via
   `crates/providers`; no new third-party code enters the build graph, and nothing ships in the
   binary).
9. `i18n::es_mirrors_en_exactly` (existing) covers EN/ES parity for the new keys.

## 8. Goal & success criteria

Make the `/models` modal tell the truth about why it could not list models, and offer the remedy
that actually applies.

- A 401/403 or missing-key fetch failure renders a step naming `/connect` and `/key` and offering
  no input box — asserted by test.
- A transport failure still offers manual entry (AC 6 of #36) **and** a retry key — asserted by test.
- `Ctrl+R` re-runs the fetch in place, bumping the nonce — asserted by test.
- A typed id reports "not verified against {provider}"; a picked id reports the unchanged string —
  asserted by test.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings` and
  `cargo fmt --all --check` are clean; EN/ES parity holds.

## 9. Risks & open questions

1. **Gemini's 400 for an invalid key classifies as `Fetch`.** The user gets retry + unverified
   manual instead of the credential step. This is the status quo, not a regression, and the
   `Fetch` step still shows the provider's own error text. Widening to 400 would misroute genuine
   bad-request bugs into a dead end. *Follow-up candidate: parse the provider error body.*
2. **Downcasting to `reqwest::Error` couples the TUI to the providers crate's HTTP client.** If
   `crates/providers` ever swaps clients, classification silently degrades to `Fetch` — a safe
   default (the old behaviour), not a crash. The proper fix is a typed error in `crates/providers`;
   deliberately deferred to avoid a semver-major public-API change and a merge conflict with #44.
3. **Merge order.** This lands third, after #45 and #44. #44 edits `begin_models_fetch` and
   `crates/providers/src/models.rs`; this change edits `begin_models_fetch`'s *sibling*
   `fetch_model_list` and does not touch `crates/providers` at all, so the overlap is one function
   in `app.rs`. #46 lands after and moves the modal wholesale.
4. **`models.footer_manual`'s value changes rather than its key.** A translator diffing by key
   will not see it. Acceptable: the ES value is updated in the same commit.

## 10. Follow-ups (file as issues, do not widen this PR)

- A typed `ModelsError` in `crates/providers` so classification lives where the HTTP does
  (semver-major on `list_models`).
- Parse provider error bodies so Gemini's 400 INVALID_ARGUMENT classifies as `Auth`.
- Surface the same unverified/verified distinction in the `/connect` modal's status line.
