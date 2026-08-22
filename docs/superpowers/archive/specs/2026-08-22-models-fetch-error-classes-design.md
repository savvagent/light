# Model-list fetch error classes — design

> **Status:** IMPLEMENTED — branch the `/models` modal on *why* the fetch failed: credential failures point at `/connect`/`/key`, transport failures offer a retry plus an explicitly-unverified manual entry.

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
- **(added in review)** `draw_popup` sizing its box from the **wrapped** row count rather than
  `body.len()` (issue #57), and a cap on the provider-supplied error text at the
  `fetch_model_list` boundary. Both were originally deferred; see §11.
- A new `ModelsStep::Credentials` terminal step for `MissingKey`/`Auth`, rendering the failure and
  the remedy (`/connect`, `/key <provider>`, `/model <id>`) with **no** input box.
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
   concurrently by #44, so a refactor there is a guaranteed merge conflict, and pulling it in would
   widen a modal fix into a cross-crate one. Walking the whole `chain()` rather than downcasting the
   root keeps this correct if #44 adds `.context(...)`.

   **Correction (review round).** An earlier draft of this rationale also claimed a typed error in
   `crates/providers` would be semver-major. That is overstated and is withdrawn: only *mutating*
   `list_models`' existing signature would break. An **additive** seam — a new
   `list_models_classified`, or `pub fn model_list_status(&anyhow::Error) -> Option<u16>` — is
   semver-minor under Non-Negotiable Rule 6 and needs no version bump. The honest reasons to defer
   are the #44 merge conflict and scope discipline, which are sufficient on their own.
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
5. ~~**Retry is offered on the transport step only, not the credential step.**~~ **Reversed in
   review.** The original reasoning assumed a 401/403 always means the API key is wrong. It does
   not: a corporate proxy, a WAF, an IP allowlist, or an org-level block produce the same status,
   and for those `/connect` and `/key` are exactly as useless as the retry-only modal #47 was filed
   to replace — the same defect, inverted. `Ctrl+R` is therefore offered on `Credentials` too, so a
   misclassification costs one keystroke instead of dead-ending the user. The machinery already
   existed: `ModelsStep::provider()` returns `Some` for `Credentials` and `retry_models_fetch`
   already handles it. The remedy line additionally names `/model <id>` as the second escape hatch
   (see §3.1 — that fallback genuinely survives, because `set_model` gates only on
   `is_valid_provider(provider_info.id)` against the stale snapshot).
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

Rendered by `draw_models` as two dark-gray lines — `models.credentials_hint` ("Typing a model id
can't fix this.") and `models.credentials_remedy` ("Use /connect, /key {provider}, or /model
<id>") — then a blank line, then the failure message (red). Footer is `models.footer_retry`
("Ctrl+R: retry · Esc: close"). No input box, no list, `focus: None`.

**Trusted rows are drawn first, provider text last** (revised in review). The provider-supplied
message is remote-controlled and was the thing displacing the modal's own rows; ordering the
remedy above it means any clip that survives costs the *error detail*, not the remedy. On `Manual`
the same ordering puts the prompt and the input row above the error.

**Popup sizing is fixed rather than worked around** (revised in review). The original design kept
every added string under the ~58-column inner width and left `draw_popup`'s `body.len()` sizing as
a follow-up. That is not sufficient: the *provider's* error is not a string this design controls,
and a real 401 renders 105+ characters, so on every genuine credential failure the box was sized
for four rows, rendered five, and clipped the remedy — with `focus: None` making the overflow
unreachable. `draw_popup` now measures with `Paragraph::line_count` (ratatui's
`unstable-rendered-line-info` feature), the same wrapper the render uses, and scrolls `focus` in
wrapped rows. See §11.

The hint's command syntax is taken from `parse_key_command` (`app.rs:2464`), which accepts `/key`
(list), `/key <provider>` (masked entry) and `/key <provider> clear` — **not** `/key set …`. Naming
a command that does not exist would be the same class of misdirection this issue exists to fix.

`models_step_next` for `Credentials`: `Ctrl+R → Retry`, `Esc | Enter → Close`, every other key →
`Step(self.clone())`. `models_apply_target` returns `None`, so the step can never persist anything —
and it says so by **naming** `Offline`, `Credentials` and a still-fetching `ModelList` rather than
falling through a `_` catch-all, so a future `ModelsStep` variant fails to compile here instead of
silently defaulting to "nothing to persist" in the one function where a wrong default would persist
a model the user never confirmed.

A small `ModelsStep::provider(&self) -> Option<&str>` accessor is added so the retry path can read
the provider from any step without a three-arm match at the call site. It returns `None` only for
`Offline`, which never emits `Retry`; `retry_models_fetch` returns early on `None` rather than
panicking, so an unreachable case stays inert instead of aborting the UI loop.

### 5.3 Retry

`ModelsTransition` gains `Retry`. `models_step_next` maps `Char('r' | 'R') + CONTROL → Retry` on
`Manual` (ordered before `Enter`/`Backspace`/`Char(c)`), on `Credentials`, and on a settled but
**empty** `ModelList` — an empty list is reachable from an Ollama install with nothing pulled, and
is worth another try. `'R'` is matched as well as `'r'` because `Ctrl+Shift+R` arrives as an
uppercase char with `CONTROL | SHIFT` and would otherwise fall through and type an `R` into the
model id.
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
| `models.credentials_hint` | `Typing a model id can't fix this.` |
| `models.credentials_remedy` | `Use /connect, /key {provider}, or /model <id>` |
| `models.footer_retry` | `Ctrl+R: retry · Esc: close` (credential step, and a settled empty list) |
| `models.manual_unverified` | `Type a model id — it won't be checked against {provider}` (replaces `models.manual`) |
| `models.footer_manual` | `Enter: save unverified · Ctrl+R: retry · Esc: close` (value updated) |

The ES `models.footer_manual` drops "sin verificar": with it the line is 63 columns against the
58-column inner width and the footer is rendered **without** `Wrap`, so it was hard-truncated and ES
users silently lost `Esc: cerrar`. The prompt line above it already says the id is not verified. A
new `every_footer_fits_the_popup_in_both_locales` test now bounds every `*.footer*` value in both
catalogs, so this cannot recur in a locale nobody renders in a test.

`models.manual` is removed from both catalogs — nothing references it once `draw_models` uses the
provider-aware replacement. The existing `es_mirrors_en_exactly` test in `i18n.rs` enforces parity
and is kept green.

## 6. Error handling & edge cases

| Case | Behaviour |
|---|---|
| Successful but **empty** list | Stays a `ModelList` with the "no models reported" notice — an empty list is not a failure — but now also offers `Ctrl+R`, since the usual cause (an Ollama install with nothing pulled) is fixed outside the modal and worth re-checking. |
| **Provider-supplied error text** | Reduced at the `fetch_model_list` boundary to its first line, stripped of control characters, and capped at 120 chars with an ellipsis. It is remote-controlled and unbounded (serde's `invalid_type` embeds the entire offending value), so unbounded it would fill the modal body and push the modal's own trusted rows off screen; a raw `ESC` written into a terminal cell is an escape-sequence injection. Capping at the boundary rather than at the draw site means the `/connect` modal, which consumes the same text via `.map_err(\|e\| e.message)`, inherits the bound. |
| **A fetch failure the user closes with Esc** | `push_log`ged as well as shown. `close_models` drops the step, so the modal is not a record; the transcript is what the user can scroll back to and paste when asking for help. |
| Late result for a superseded fetch | Unchanged — `models_nonce` guard drops it. Retry bumps the nonce, so a slow first response cannot overwrite the retry's state. |
| Late result while the user is typing in `Manual` | Unchanged — `handle_models_fetched` only accepts results while the step is `ModelList { fetching: true }`, so typed input is never clobbered. Retry deliberately leaves `Manual`, discarding the partial id; that is the user's explicit action. |
| Late result while the step is `Credentials` | Dropped by the same `ModelList { fetching: true }` guard, so a slow response cannot replace the credential notice with a list fetched under a key that was already refused. |
| Session lost mid-fetch | Unchanged — `dismiss_modals` clears the modal and bumps the nonce. `Credentials` needs no special handling. |
| Ollama (keyless) | **Always `Fetch`**, forced rather than classified. Ollama takes no key, so no failure of its is repairable by `/connect` or `/key` — not even a 401 from a proxy in front of it — and routing one to the credential step would invent a remedy that does not exist for this provider. Retry + unverified manual is correct: `ollama serve` may simply not be running yet. |
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
   `Fetch`. Requires `wiremock` as a `crates/tui` **dev**-dependency — a `[dev-dependencies]`
   section is added to `crates/tui/Cargo.toml` (the crate has none today). `wiremock 0.6` is already
   in `Cargo.lock` via `crates/providers`, so no new third-party code enters the graph;
   `cargo build --workspace` and the shipped binary are unaffected because dev-dependencies are
   compiled only for test targets.
9. `i18n::es_mirrors_en_exactly` (existing) covers EN/ES parity for the new keys.
10. Two `ratatui` `TestBackend` render assertions, **both built through `handle_models_fetched`
    from a real `reqwest` error** rather than from a hand-written state: a wiremock 401 for
    `Credentials`, a refused connection for `Manual`. Each asserts the trusted rows are on screen
    (`/connect`, `/key openai`, `/model <id>`; the unverified prompt plus text typed into the box)
    **and** that the provider's message survives in full, compared against a whitespace-collapsed
    rendering of the popup so the assertion does not have to predict where the wrapper broke the
    line. Hand-written states are what let the first version of these tests pass while the shipped
    modal clipped its own remedy.

**Added in the review round:**

11. `draw_popup` sized from wrapped rows: a body of `[<200-column line>, "TAIL-MARKER"]` must render
    `TAIL-MARKER` and the pinned footer. Fails against `body.len()` sizing.
12. `draw_popup` focus scrolling counted in wrapped rows: a focused row below 40 wrapping lines
    stays on screen in a 12-row terminal.
13. `fetch_with_key(provider, None, locale)` → `MissingKey`. The classification boundary the whole
    change rests on had no test; the arm is split out of `fetch_model_list` precisely so it is
    reachable without mutating the process environment (`resolve_key` reads `OPENAI_API_KEY`).
14. `class_for_provider("ollama", <a real 401>)` → `Fetch`, and `class_for_provider("openai", …)` →
    `Auth` on the same error, so the forcing is scoped rather than defeating classification.
15. `summarize_provider_error`: first line only, control characters stripped, capped at 120 chars
    plus an ellipsis, and no split mid-character on multi-byte input.
16. `/model <id>` reports the unverified status — a claim with nothing behind it until now.
17. A **late `Err`** result does not clobber a `Manual` step's typed input. Every pre-existing
    stale-result test used `Ok`, so narrowing the guard to `result.is_ok() && …` survived mutation.
18. A fetch failure appears in `app.log` and is still there after `close_models`.
19. `Ctrl+R` on `Credentials` and on a settled empty list; `Ctrl+Shift+R` on `Manual` retries rather
    than typing an `R`.
20. `every_footer_fits_the_popup_in_both_locales` bounds every `*.footer*` value in EN and ES; a
    Spanish-locale render asserts `Esc: cerrar` and `Ctrl+R: reintentar` survive.

All four network tests build their client explicitly with `reqwest::Client::builder().no_proxy()`
plus a connect timeout and a request timeout. `reqwest::get` honours `http_proxy`/`HTTP_PROXY` and
has neither — under an exported proxy two of these tests were observed passing a 200 through to
`expect_err`, and against a sandbox that DROPs rather than RSTs the connection to port 1 the
transport test blocked on the kernel SYN-retry budget (~130s) with nothing to bound it.

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
   `crates/providers` ever swaps clients, wraps with a bare `anyhow!`, or the two crates resolve
   different `reqwest` majors, classification silently degrades to `Fetch` — a safe default (the old
   behaviour), not a crash, but a fully green suite would not notice: every classification test
   builds its `reqwest::Error` inside `crates/tui`. Nothing exercises the cross-crate assumption.
   Closing it needs a test that points `OPENAI_BASE_URL` at a wiremock 401 (which `validate_base_url`
   permits, since it is loopback `http`) and asserts `fetch_model_list("openai", Some("k"), …)`
   yields `Auth`. That requires mutating the process environment, which in Rust 2024 is `unsafe` and
   races every other test in the binary that reads a key var, so it is **deferred to a follow-up**
   rather than done here with a global test mutex. The `.context("listing models")` test is the
   partial guard: it proves chain traversal survives wrapping, which is the most likely way this
   breaks when #44 lands. The proper fix is an additive typed seam in `crates/providers` (semver-
   minor — see Assumption 1), deferred for merge-conflict and scope reasons.
3. **Merge order.** This lands third, after #45 and #44. #44 edits `begin_models_fetch` and
   `crates/providers/src/models.rs`; this change edits `begin_models_fetch`'s *sibling*
   `fetch_model_list` and does not touch `crates/providers` at all, so the overlap is one function
   in `app.rs`. #46 lands after and moves the modal wholesale.
4. **`models.footer_manual`'s value changes rather than its key.** A translator diffing by key
   will not see it. Acceptable: the ES value is updated in the same commit.
5. **`ratatui`'s `unstable-rendered-line-info` feature is now enabled** for `Paragraph::line_count`.
   Its own docs warn the wrapping design is not stable, so a ratatui upgrade could change the
   measurement. Accepted deliberately: the alternative is a hand-rolled wrap counter, which would
   reintroduce exactly the measure/render disagreement the fix exists to remove. Using ratatui's own
   wrapper means the two can only ever move together, and the `draw_popup` tests fail loudly if they
   do not.

## 10. Follow-ups (file as issues, do not widen this PR)

- A typed `ModelsError` in `crates/providers` so classification lives where the HTTP does
  (semver-major on `list_models`).
- Parse provider error bodies so Gemini's 400 INVALID_ARGUMENT classifies as `Auth`.
- Surface the same unverified/verified distinction in the `/connect` modal's status line.
- ~~`draw_popup` sizes its box from `body.len()` rather than the wrapped line count.~~ **Done in
  this change** (issue #57) — it turned out to be load-bearing rather than cosmetic: it fires on
  every real 401, so the headline behaviour did not render without it.
- A cross-crate classification test (see Risk 2), pointing `OPENAI_BASE_URL` at a wiremock 401.

## 11. Deviations from the reviewed design

Recorded because each is a real design change made after the spec was approved, in response to
review:

| # | Deviation | Why |
|---|---|---|
| 1 | `draw_popup` sizes itself from the **wrapped** row count (issue #57), and `focus` scrolls in wrapped rows. `ratatui`'s `unstable-rendered-line-info` feature is enabled. | Originally a follow-up. The provider's error is not a string this design controls; a real 401 is 105+ characters, so the credential step's remedy was clipped on **every** genuine credential failure and `focus: None` made it unreachable. The headline behaviour did not render without this. |
| 2 | Provider error text is reduced to one control-free line and capped at 120 chars at the `fetch_model_list` boundary. | The text is remote-controlled and unbounded. Unbounded it displaces the modal's own trusted rows, which turns a model picker into a credential-phishing surface; the cap belongs at the boundary so the `/connect` modal inherits it. |
| 3 | Trusted rows are drawn **before** the provider's error on both failure steps. | Defence in depth for deviation 1: whatever a short terminal clips, it must not be the remedy or the input box. |
| 4 | `Ctrl+R` is offered on `Credentials` (reverses Assumption 5), the remedy names `/model <id>`, and the step gets its own footer. | A 401/403 is not proof the key is wrong. Without a retry the step is a dead end pointing at two commands that cannot help — #47's own defect, inverted. |
| 5 | `Ctrl+R` is offered on a settled but empty `ModelList`; `Ctrl+Shift+R` retries instead of typing an `R`. | Both are one-line completions of the retry affordance the change introduces. |
| 6 | The `Err` arm `push_log`s the classified failure. | `close_models` drops the step, so Esc erased the only copy of the error. Inconsistent with `clear_key`/`submit_key_entry`/`list_keys` in the same file. |
| 7 | `models_apply_target` names its `None` variants instead of using `_`. | It is the one function where a wrong default persists a model the user never confirmed; the compiler should carry that, not luck. |
| 8 | ES `models.footer_manual` drops "sin verificar"; a width test bounds every footer in both locales. | 63 columns against a 58-column inner width, rendered without `Wrap`, hard-truncated — ES users lost `Esc: cerrar`. Invisible because every render assertion ran in EN. |
| 9 | `fetch_model_list`'s no-key arm and the Ollama class forcing are split into `fetch_with_key` and `class_for_provider`. | Both were untestable in place (`resolve_key` reads the process environment; the Ollama base URL is hardcoded), and mutation testing confirmed both survived. |
| 10 | The network tests build their client with `no_proxy()` and explicit timeouts. | Reproduced, not hypothetical: an exported `http_proxy` made two of them pass a 200 through to `expect_err`. |
