# /connect modal flow — design

> **Status:** DRAFT — single guided modal replacing the text-command provider setup.

> **Implements:** https://github.com/savvagent/light-factory/issues/35

## 1. Brief

The provider setup surface added in #33 is text-command-based: the user must type
`/provider <name>`, `/key <provider>`, and `/model <id>` into the activity log, and read the result
as log lines. That is clunky. This issue replaces it with a single guided modal flow opened by
`/connect`, which walks the user through provider → key (when needed) → model.

Acceptance criteria (quoted from the issue):

1. Typing `/connect` (in place of `/provider`) opens a modal.
2. The modal shows a **"Connect a provider"** header and a list of supported providers.
3. Selecting a provider that is **already connected** changes the modal to list the **models offered
   by that provider** and lets the user pick one.
4. Selecting a provider that is **not connected** changes the dialog to show the heading **"API Key"**
   and lets the user enter a key.
5. On submitting the key: the key is saved, we **test the key**, and **fetch the available models**
   using it.
6. The available models are then shown, and the user can select one.
7. Every modal shows **Esc** to close and **Enter** to submit.

## 2. Assumptions

1. **`/provider` is replaced, not aliased.** "Instead of `/provider`" is read as replace: the
   `/provider` command is removed and `/connect` takes its place. `/model` and `/key` remain
   unchanged as power-user fallbacks and their persist/rebuild logic is reused internally by the
   modal flow. Rationale: keeps the issue's scope on the modal; removing the other two commands would
   be unrequested surface loss.
2. **"Connected" means "a key is resolvable (or Ollama enabled)", not "currently active".** A
   keyed provider is *connected* when a key is resolvable (env var or keyring). `ollama` takes no
   key: it always routes to the model fetch, and its connected marker simply reflects whether
   `LIGHT_OLLAMA=1` is set (informational only). The active/selected provider is irrelevant to the
   connected check.
3. **"Connected" is a storage predicate, not a validity predicate.** A stored-but-unverified key
   still counts as connected; we do *not* call `list_models` merely to mark a row connected (that
   would add a network call per provider at modal-open). The key is validated only when the user
   picks that provider and we fetch its models. A bad-but-saved key therefore renders as connected
   and surfaces at the fetch step as a recoverable error (§6).
4. **Model listing is a free function in `crates/providers`, not a `Provider` trait method.** A
   `Provider` is pinned to a single model and to the *active* selection, but `list_models` needs no
   model and must run for a provider that may not be active (and with a freshly typed key). A free
   function decouples setup-time listing from runtime completion and avoids exposing per-provider
   constructors. All additions are public-but-additive (new module + free functions), no semver bump
   (Non-Negotiable Rule 6).
5. **"Test the key" and "fetch the models" are the same call.** Listing models with the key both
   validates it (auth error ⇒ bad key) and returns the list; no separate "ping"/"whoami" endpoint.
   On failure, the key remains saved (the user typed it; we do not silently discard it) and the modal
   offers a replace-key path (§6).
6. **Model list is fetched off the UI loop.** Like `/ask` (`app.rs::ask`), the list call runs in a
   `tokio::spawn` task that posts a new `UiEvent` back; the modal shows a "fetching…" state
   meanwhile. The TUI loop never blocks on network I/O.
7. **Model choice persists and activates the provider.** Selecting a model writes
   `settings.models[provider]` **and** sets `settings.provider = Some(provider)` (so the chosen
   provider becomes active), then saves settings and calls the existing `rebuild_provider()`. Without
   the `settings.provider` write, picking a model for a non-active provider would persist it but not
   switch to it.

## 3. Goal & Success Criteria

Goal: a user can, from the connected TUI screen and without knowing command syntax, open one modal
and end with a provider that is active, has a working key (or a reachable Ollama server), and a
chosen model.

- [ ] `/connect` opens the modal; `/provider` is gone; the connected header hint advertises
      `/connect`.
- [ ] The modal lists the five supported providers and correctly marks each connected or not.
- [ ] Connected provider → model list rendered from a live `list_models` call; choosing one persists
      **and activates** the provider.
- [ ] Unconnected keyed provider → "API Key" masked entry; Enter saves the key, lists models with it,
      and proceeds to the model list; a bad key shows a recoverable error and does not crash.
- [ ] Ollama → model list from the local server, no key step.
- [ ] Every step shows "Esc: close · Enter: submit" and honors those keys.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
      `cargo fmt --all --check` clean; ES mirrors EN; no key value appears in any log/status/error.

## 4. Scope

**In:**
- `/connect` command (opens the modal) and removal of `/provider`.
- A connect modal state machine in `crates/tui/src/app.rs` (provider list → key entry → model list)
  with keyboard handling (Esc close / Enter submit, up/down to navigate lists).
- A `models` module in `crates/providers` that lists model ids for anthropic, openai, gemini,
  deepseek, and ollama (see §5.1).
- The "test key ⇒ fetch models" flow, run off the UI loop, with a "fetching…"/error intermediate
  state and a replace-key recovery path.
- A public `resolve_key(provider, store) -> Option<String>` helper in `crates/tui/src/selection.rs`
  so the modal can read the saved key of a connected provider to fetch its models.
- EN + ES i18n for every new string.

**Out:**
- Any change to `protocol`, `server`, `persistence`, `web/`, or the auth spine.
- Removing `/model` or `/key` commands (kept as fallback).
- Persisting base-URL overrides (still env-only).
- Mid-*turn* provider switching inside a live engine session (unchanged from #33: takes effect on the
  next engine session).
- Model *search/filtering* or pagination — the list is rendered as-is, up/down selects.

## 5. Approach

### 5.1 Model listing (`crates/providers/src/models.rs`)

A new `models` module exposes four functions; the `_at` variants take an explicit base URL so they
are testable against a local mock server (the existing per-provider test pattern), and the plain
variants resolve the base URL (env override → production default, reusing the crate's existing
`base_url_var`/`resolve_base_url`/`default_base` machinery) then delegate:

```rust
/// List model ids for a keyed provider against `base_url`. Normalizes ids, stable-sorts, dedups.
/// `pub(crate)`: takes an already-validated base URL, so external callers cannot hit it with an
/// unvalidated host. The public, validating entry point is [`list_models`].
pub(crate) async fn list_models_at(provider: &str, base_url: &str, key: &str) -> anyhow::Result<Vec<String>>
/// Resolve the base URL for `provider` and delegate to `list_models_at`.
pub async fn list_models(provider: &str, key: &str) -> anyhow::Result<Vec<String>>
/// List model ids from the local Ollama server at `base_url`. `pub(crate)` for the same reason.
pub(crate) async fn list_ollama_models_at(base_url: &str) -> anyhow::Result<Vec<String>>
/// Resolve the Ollama base URL and delegate to `list_ollama_models_at`.
pub async fn list_ollama_models() -> anyhow::Result<Vec<String>>
```

| Provider | Endpoint | Auth header(s) | Id normalization |
|---|---|---|---|
| anthropic | `GET /v1/models` | `x-api-key`, `anthropic-version: 2023-06-01` | as returned |
| openai | `GET /v1/models` | `Authorization: Bearer <key>` | as returned |
| gemini | `GET /v1beta/models` | `x-goog-api-key: <key>` | strip `models/` prefix from `name` |
| deepseek | `GET /models` | `Authorization: Bearer <key>` | as returned |
| ollama | `GET /api/tags` | none | extract `name` verbatim (incl. `:tag`) |

Notes: the `anthropic-version` header is **required** by the Anthropic API on every request including
`/v1/models` (the `complete` path already sends `2023-06-01`); omitting it returns 400 with a valid
key. Gemini `name` fields are `models/<id>`, and storing them verbatim would produce a doubled
`models/` prefix in the generateContent path (`gemini.rs:108`), so they are stripped. Ollama
`/api/tags` returns `{"models":[{"name":"llama3.2:latest", …}]}`; the `name` (including the `:tag`)
is extracted as the id so the user can pick a tagged model. Non-2xx / auth errors propagate as
`anyhow::Error`. The functions return every id the API reports — no chat-model filtering — stable-
sorted and deduped; the user chooses, and a non-chat id is their responsibility.

**Trust boundary (security invariant, must not regress).** `list_models` / `list_ollama_models`
resolve a `*_BASE_URL` env override through the *same* `validate_base_url` gate used by provider
construction (`base_url.rs`), and their HTTP client is built with `build_http_client` — redirects
**off** — so a 3xx is rejected rather than followed. Rationale: the `*_BASE_URL` value receives the
API key, so it is validated before any request is made, exactly as `build_remote` does today; a
bare `reqwest::Client::new()` against the raw env value would send the key to an unvalidated or
redirected host. The mock tests in §7 must assert that a 3xx and an invalid override are rejected
(no key sent).

### 5.2 Connect modal state machine (`crates/tui/src/app.rs`)

A modal struct, separate from the existing `Mode` (which is a full-screen mode, not an overlay):

```rust
struct ProviderRow { id: String, connected: bool }          // takes_key == id != "ollama"

enum ConnectStep {
    ProviderList { rows: Vec<ProviderRow>, selected: usize },
    KeyEntry    { provider: String, input: String },
    ModelList   { provider: String, models: Vec<String>, selected: usize,
                  fetching: bool, error: Option<String>, from_key: bool },
}
```

`App` gains `connect: Option<ConnectStep>` and `connect_return: Mode`. Entering `/connect` builds
`ProviderList` from the existing `PROVIDER_NAMES` constant, marking each keyed provider connected via
`key_status(id) != None` and `ollama` via the `LIGHT_OLLAMA` env flag. The `ProviderList` rows are
self-contained (id + connected flag), so the pure transition function needs no external state.

The step-to-step transition is extracted into a **pure function** so it is unit-testable without a
terminal, keyring, or network:

```rust
enum ConnectTransition { Step(ConnectStep), Close }
fn connect_step_next(step: &ConnectStep, key: KeyEvent) -> ConnectTransition
```

Routing rules encoded in `connect_step_next` (no network effects):

- **ProviderList**: Enter → `ModelList{ fetching:true }` for a connected keyed provider or for
  `ollama`; Enter on an unconnected keyed provider → `KeyEntry`. Esc → `Close`. Up/Down move
  `selected`.
- **KeyEntry**: Enter → (caller saves the key, then) `ModelList{ fetching:true, from_key:true }`;
  Esc → `ProviderList`. Printable chars/Backspace edit `input`; the caller masks rendering.
- **ModelList**: Enter → `Close` (the caller applies the chosen model, §5.3). Esc → `KeyEntry`
  when the provider `takes_key` and (`from_key || error.is_some()`) — i.e. a failed fetch on a keyed
  provider drops the user onto key replacement — otherwise `ProviderList` (so a failed Ollama fetch
  returns to the provider list, since Ollama has no key). Up/Down move `selected`.

The caller (`handle_key`) applies `ConnectTransition::Step`; for transitions into a `fetching`
`ModelList` it spawns the `list_models` task off-loop (key = the just-typed key, or
`resolve_key(provider, store)` for a connected provider; ollama uses `list_ollama_models`). Results
return via a new `UiEvent::Models { provider, result: Result<Vec<String>, String> }` guarded by a
nonce so a stale result from a closed modal is ignored.

`KeyEntry` owns its own `input: String` buffer and renders through a shared masking helper (extract
the `"*".repeat(n)` logic from `draw_key` into a pure `fn mask(&str) -> String`), so no key value is
ever drawn. `connect_return: Mode` plays the role `key_return: Mode` plays today for restoring the
screen on close.

Rendering is a centered ratatui `Clear` + bordered `Block` popup drawn over the current screen, with
a localized footer `Esc: close · Enter: submit` on every step (ModelList shows `Esc: back · Enter:
select` when not fetching). The connected screen's existing `keyring` lookups at modal-open (`store.get`
× 4) block the UI thread briefly, matching the existing `/provider` listing — not a regression.

### 5.3 Command wiring

`run_command` drops the `/provider` branch and adds `/connect` (with a `parse_connect_command`
helper for symmetry with the existing parsers). Dropping `/provider` orphans `set_provider` and
`list_providers` (`app.rs:520`/`531`), which the plan must delete along with their now-unused i18n
keys — the clippy `-D warnings` gate treats dead code as an error. (`is_valid_provider` stays: it is
still used by `set_model`.) Model application reuses and **supersedes**
`set_model`: on ModelList+Enter the caller sets `settings.models[provider] = model`, **and**
`settings.provider = Some(provider)`, saves settings, calls `rebuild_provider()`, and closes the
modal. (`set_model`, which keys off the *current* active id and never sets `settings.provider`, is
insufficient for a non-active provider; the modal path sets both.) `store.set` reuse is unchanged.
`hint.connected` is updated to advertise `/connect` (the old `/provider` mention is removed).

## 6. Error Handling & Edge Cases

- `list_models` auth failure (bad key) → `ModelList.error` localized "couldn't fetch models — check
  the key"; the saved key is kept; Esc drops to `KeyEntry` so the user can replace it in-modal.
- `list_models` network timeout/unreachable (esp. Ollama) → same localized error; keyed providers
  offer the replace-key path, Ollama Esc returns to `ProviderList`.
- Hung endpoint during fetch → the modal stays in "fetching…" (no new timeout mechanism; the
  providers' client/timeout behavior is unchanged from `complete`), but Esc still closes/unwinds at
  any point, so the user is never stuck — matching `/ask`.
- Blank key on Enter → no keyring write, stay on `KeyEntry` with a hint (matches #33's blank-key
  no-op).
- Keyring unavailable (`store.set` errors) → localized error, stay on `KeyEntry`.
- Empty model list returned by an API → `ModelList` shows "no models reported"; Enter is a no-op.
- Esc unwinds one step (ModelList → KeyEntry/ProviderList → Close); `Close` clears `connect` and
  restores `connect_return`.
- A stale `UiEvent::Models` arriving after the modal closed is ignored (nonce guard).
- `local`/`scripted` never appear in the list.

## 7. Testing Approach

- **Providers:** one `list_models_at` test per provider against the existing local mock HTTP server
  pattern (`server.uri()`), asserting the correct path + auth headers (incl. `anthropic-version`) and
  id parsing/normalization (Gemini `models/` strip, Ollama `name` extraction, dedup, stable sort); an
  auth-error response → `Err`; a 3xx response → rejected (redirects off, no key sent) and an invalid
  `*_BASE_URL` override → rejected by `validate_base_url`; `list_models`/`list_ollama_models` wrapper
  tests assert env-override → default base-url resolution (no live network).
- **TUI modal logic:** `connect_step_next` is pure — test Esc/Enter/up/down, connected vs unconnected
  routing, ollama skips the key step, blank-key no-op, the Esc-after-failed-fetch → `KeyEntry` rule,
  and back-navigation. `mask` is tested for never echoing the input.
- **i18n:** EN + ES parity test (existing `i18n.rs` mechanism) covers every new key.
- **Integration gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo fmt --all --check`.

## 8. Risks & Open Questions

- **Medium — four bespoke models endpoints + one local one.** OpenAI/DeepSeek/Gemini/Anthropic each
  have a different path and auth; Ollama is a fifth. Each is a small isolated function; the mock
  tests in §7 pin each.
- **Medium — per-provider response shapes differ** (Anthropic `{data:[{id}]}`, Gemini
  `{models:[{name}]}`, OpenAI/DeepSeek `{data:[{id}]}`). Parsing is per-provider and fails closed to
  a modal error rather than a wrong model id.
- **Low — base-URL override ending in `/v1` doubles the prefix** for the OpenAI/DeepSeek `list_models`
  path, identical to the pre-existing `join_url` behavior for `/v1/chat/completions`; accepted, not
  changed.
- **Low — env-only keys show as connected but can't be edited in-modal** (env wins over keyring); the
  model list still works. Recorded as deliberate (§2.2).
- **None — semver.** New module + free functions are additive; the rest is TUI-internal.
- **Open — should `/provider` be kept as an alias?** Assumed removed per §2.1. If muscle memory
  matters, a one-line alias is trivial; flagged rather than built unrequested.
