# /models command — design

> **Status:** IMPLEMENTED — a `/models` modal scoped to the active provider, reusing #35's fetch plumbing.

> **Implements:** https://github.com/savvagent/light-factory/issues/36

## 1. Brief

`/model <id>` requires the user to already know the exact model id. #35 shipped a `/connect`
modal whose final step lists a provider's models and lets the user pick one. This issue promotes
that model-selection step to a standalone command: `/models` opens a modal listing the models of
the **currently active provider** (not a freshly connected one) and lets the user pick one, reusing
the fetch plumbing and modal-framework semantics from #35 (`crates/providers/src/models.rs`,
`resolve_key`, Esc/Enter list selection, off-loop fetch).

Acceptance criteria (quoted from the issue):

1. Typing `/models` opens a modal with a **"Select a model"** header.
2. The modal lists the models offered by the currently active provider (via the fetch plumbing from
   #35), with the currently selected model pre-highlighted.
3. Selecting a model persists it as that provider's model (`settings.models[<provider>]`) and
   rebuilds the active provider; the connected header reflects the change immediately (`/ask`
   immediately, the next engine session — the existing switch semantics from #23).
4. **Esc** closes the modal without changing anything; **Enter** confirms the highlighted selection.
5. When the active provider is offline (`LocalProvider`, nothing configured or a named provider
   missing its key), the modal shows a notice instead of a list and offers no selection.
6. When the model list cannot be fetched (network error, bad key), the user can fall back to typing
   a model id manually — preserving today's `/model <id>` behavior.

## 2. Assumptions

1. **"Currently active provider" = the provider the connected screen is showing**, i.e.
   `self.provider_info.id` at modal-open (`app.rs:provider_info`), **not** the last-stored
   `settings.provider` preference. Rationale: the modal is scoped to "the provider I'm already
   using", which is the live selection (env/keyring/preference resolved by `crate::selection`),
   exactly what the connected header renders (`draw_connected` → `provider_info.display()`).
2. **"Currently selected model" = `provider_info.model`** (the effective model — explicit override,
   stored preference, or the provider default constant). When that id appears in the fetched list it
   is pre-highlighted; otherwise the highlight falls back to index 0. Rationale: `provider_info.model`
   is what the provider is actually pinned to, so it is the thing "selected" today.
3. **Ollama is covered in this issue.** `list_ollama_models()` already exists (#35) and the active
   provider may be Ollama (`LIGHT_OLLAMA=1` or a stored `ollama` preference). Excluding it would make
   `/models` silently useless on a supported, keyless provider. The open question is answered in
   favor of coverage — it costs one branch, no new plumbing.
4. **The manual fallback is in-modal, and `/model <id>` is preserved.** On a fetch failure the modal
   switches to a manual model-id entry (unmasked — a model id is not a secret) that applies exactly
   what `/model <id>` does (persist + rebuild). The `/model` command itself is untouched, so both
   paths satisfy AC 6. Rationale: "the user can fall back to typing a model id manually" implies the
   modal offers typing, and the issue explicitly says to preserve the `/model <id>` behavior.
5. **Offline is detected by `provider_info.offline.is_some()`** (the `LocalProvider` marker), not by
   inspecting the id string. Rationale: the selection layer sets `offline` exactly when the offline
   `LocalProvider` was built (`selection.rs::build_provider`), so it is the authoritative signal; the
   id is a display field.
6. **A successful-but-empty model list is not a fetch failure.** It renders the existing "no models
   reported" notice with Enter a no-op and Esc closing, matching #35's model-list step. Only a real
   error (network/auth) routes to the manual entry. Rationale: an empty list is a distinct, honest
   "provider returned nothing" state; treating it as an error would wrongly imply the key is bad.
7. **The manual entry is not reachable from a successful, non-empty list.** Adding a model outside
   the fetched list when listing works is out of scope (the issue's fallback is specifically the
   *cannot-fetch* case); `/model <id>` remains the power-user path for that.

## 3. Goal & Success Criteria

Goal: from the connected screen, a user can type `/models`, see the models of the provider they are
already using, pick one (or type one when the list cannot be fetched), and have it take effect
immediately — without ever typing an exact model id from memory.

- [ ] `/models` opens a "Select a model" modal on the connected screen; typed elsewhere it shows a
      localized "after you sign in" error (mirroring `/connect`/`/ask`).
- [ ] A live provider fetches its model list via the existing #35 plumbing and pre-highlights the
      current model; Enter persists `settings.models[<provider>]`, rebuilds the provider, and the
      connected header updates immediately.
- [ ] Esc closes without changing settings; Enter confirms the highlighted selection.
- [ ] Offline → a notice, no list, no selection; Ollama → its local models, no key.
- [ ] Fetch failure → a localized error plus an inline manual model-id entry; `/model <id>` still
      works.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
      `cargo fmt --all --check` clean; ES mirrors EN; no key/model-id appears in any log/status.

## 4. Scope

**In:**
- A `/models` command (bare, no argument) and a `models` modal in `crates/tui/src/app.rs` with a
  pure `models_step_next` transition function and `ModelsStep`/`ModelsTransition` types.
- Reuse of `list_models`/`list_ollama_models` and `resolve_key` (#35); a new `UiEvent::Picker` +
  `models_nonce` stale-result guard; off-loop fetch mirroring `begin_fetch`.
- Manual model-id entry shown on fetch failure (persist + rebuild, the `/model <id>` behavior).
- EN + ES i18n for every new string; a `help.commands.models` entry in the help modal.

**Out:**
- Any change to `protocol`, `server`, `persistence`, `web/`, the auth spine, or `crates/providers`.
- Connecting a new provider or supplying an API key (that is `/connect`); switching provider (`/connect`).
- Removing or changing `/model <id>`, `/key`, `/connect`.
- Caching/persisting a model list (fetched live, not stored).
- Model search/filtering/pagination; manual entry from a successful non-empty list (§2.7).
- Mid-*turn* provider switching inside a live engine session (unchanged: next session).

## 5. Approach

### 5.1 Modal state machine (`crates/tui/src/app.rs`)

A standalone modal, distinct from `ConnectStep` (which is a multi-step provider→key→model flow).
`/models` is a single-purpose modal with three steps:

```rust
enum ModelsStep {
    ModelList { provider: String, models: Vec<String>, selected: usize, fetching: bool },
    Manual     { provider: String, input: String, error: Option<String> },
    Offline,
}
```

`App` gains `models: Option<ModelsStep>`, `models_return: Mode`, and `models_nonce: u64` (a fourth
counter alongside `nonce`/`device_nonce`/`connect_nonce`) as the stale-result guard. Entering
`/models` reads `provider_info.id` and `provider_info.offline`:

- offline → `Offline` (no fetch).
- otherwise → `ModelList { fetching: true }` and spawn the fetch off-loop.

The step-to-step transition is a **pure** function (testable without a terminal/keyring/network):

```rust
enum ModelsTransition { Step(ModelsStep), Close, Apply }
fn models_step_next(step: &ModelsStep, key: KeyEvent) -> ModelsTransition
```

Routing rules (no network effects):

- **Offline**: Esc/Enter → `Close`; anything else → stay. No selection is ever produced.
- **ModelList (fetching)**: Esc → `Close` (cancel; the nonce guard drops the late result); everything
  else → stay.
- **ModelList (not fetching, non-empty)**: Esc → `Close`; Enter → `Apply`; Up/Down move `selected`
  (wrapping, reusing `cycle_index`).
- **ModelList (not fetching, empty)**: Esc → `Close`; Enter/Up/Down → stay (rendered as "no models
  reported").
- **Manual**: Esc → `Close`; Enter with a non-empty trimmed id → `Apply`; Backspace/printable chars
  edit `input` (unmasked — a model id is not a secret).

The caller (`handle_models_key`) maps `Close` → `close_models()` (never applies), `Apply` →
`apply_and_close_models()`, `Step(next)` → `self.models = Some(next)`. This explicit `Close`/`Apply`
split avoids the connect flow's implicit "inspect the step to decide" trick, because here Esc must
close a *selectable* `ModelList` without applying — unlike connect, where Esc always unwinds one
step rather than closing from the model list.

### 5.2 Fetch and apply

`enter_models` spawns the same fetch as connect's `begin_fetch`, but posts a new `UiEvent::Picker`
variant (the connect variant is routed by `connect_nonce`; a second variant keeps the two flows'
routing independent and unambiguous):

```rust
Picker { nonce: u64, provider: String, result: Result<Vec<String>, String> }
```

The fetch body is the existing shape: `list_ollama_models()` for `"ollama"`, else
`resolve_key(provider, store)` → `list_models(provider, key)`, else `Err("no API key for …")`.
`handle_picker` ignores a stale nonce or a provider mismatch, and on success fills `models`,
computes `selected` as the index of `provider_info.model` (else 0), and clears `fetching`; on
failure it swaps the step to `Manual { input: "", error: localized }`.

`apply_and_close_models` extracts the choice (the highlighted model, or the trimmed manual id),
closes, then — only for a real choice — delegates to `persist_model`, which writes
`settings.models[provider] = model`, saves, calls `rebuild_provider()`, and sets `status.model_set`.
It deliberately does **not** write `settings.provider` (the provider is already active), unlike
`apply_and_close_connect` which must activate a freshly connected provider.

**Save failures roll back.** `persist_model` stages the insert, and on a save failure restores the
previous value (or removes the entry) and reports through `self.error`, not `self.status`. Without
the rollback the rejected model stays in the in-memory `Settings`, which is the input to *every*
later save and rebuild — so a subsequent unrelated `/lang` or `/key` would silently persist and
activate the model the user was just told had failed. `persist_settings`/`persist_model` are shared
with `/model`, `/lang`, and the connect apply path, which previously discarded the save `Result`
outright.

Rendering reuses the centered `Clear`+`Block` popup from `draw_connect`, factored into a shared
`draw_popup(frame, area, title, body, footer, focus)` helper to avoid duplicating the
rect/clear/render block. The title is the localized "Select a model"; the footer is per-step
(Esc/Enter + up/down arrows).

`draw_popup` pins the footer to the bottom of the popup and scrolls `body` so that the row named by
`focus` (the highlighted model) stays visible. Without this, a provider returning more models than
the terminal has rows renders the pre-highlighted row and the footer off-screen — arrow keys appear
frozen while the pending selection walks invisibly, and Enter commits a model the user never saw.
Both modals pass `focus` for their list steps. The popup height is clamped in `usize` before the
`u16` cast, so a remote-supplied list cannot overflow it.

The `Offline` step renders `crate::provider::offline_notice(locale, reason)` above the
"use /connect" hint, so the notice names the real reason. `models.offline` alone would report "no
active provider" for `NamedProviderMissingKey` and `BaseUrlRejected`, where a provider *is* named
and the real problem is a missing key or a rejected base URL.

### 5.3 Command wiring

`run_command` adds a `parse_models_command` branch (mirroring `parse_connect_command`; bare
`/models`, word-boundary-checked so `/modelsx` does not match and `/model`'s parser is unaffected —
`/models` fails `parse_model_command`'s word-boundary check). `/models` is gated to
`Mode::Connected` like `/connect`/`/ask`. The help modal's `help_lines` `commands` section gains a
`help.commands.models` entry; EN + ES keys are added together (parity test-enforced).

## 6. Error Handling & Edge Cases

- Fetch auth failure / network error → `Manual` with a localized error (reusing `connect.fetch_error`)
  and an empty input; Enter with a blank id is a no-op; Esc closes unchanged.
- Stale `UiEvent::Picker` after close → ignored (`models_nonce` bump in `close_models`).
- Hung endpoint → `ModelList{fetching}` stays "fetching"; Esc still closes (matching `/ask`/`/connect`).
- Empty list on success → "no models reported"; Enter no-op, Esc closes (§2.6).
- Offline provider (nothing configured / named provider missing key / base-URL rejected) → `Offline`
  notice, Enter/Esc close, no selection.
- A live remote whose key is somehow unresolvable (defensive) → the fetch yields `Err`, landing on
  `Manual` with an error; the user can type an id but the rebuild will still be offline.
- No secret is ever rendered: model ids are not secrets; API keys never enter this modal (fetched via
  `resolve_key`, never displayed).

## 7. Testing Approach

- **Pure transitions:** `models_step_next` — Offline Esc/Enter→Close; fetching Esc→Close + Enter
  no-op; list Esc→Close, Enter→Apply, Up/Down wrap, empty-list Enter no-op; Manual Esc→Close,
  Enter-with-id→Apply, blank-Enter no-op, Backspace/Char editing.
- **`parse_models_command`:** `/models`, `/models   ` → true; `/modelsx`, `/model gpt`, `/connect` →
  false.
- **`handle_picker`:** stale nonce ignored; matching nonce fills models and pre-highlights the current
  model (index 0 when absent); error → `Manual` with a localized error.
- **Apply path:** `handle_models_key` Enter on a list applies to `settings.models` + rebuilds (assert
  `settings.models` and `provider_info.model`); Esc leaves settings unchanged; manual entry applies
  the typed id.
- **i18n:** the existing EN/ES parity test covers every new key; `help_lines` tests cover the new
  command entry.
- **Integration gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo fmt --all --check`.

### 5.4 Config-path seam

Testing the apply path requires that a test never write the developer's real
`$XDG_CONFIG_HOME/light-factory/config.json`. `settings.rs` therefore exposes its path-taking
accessors (`path`, `load_at`, `save_at`, all `pub(crate)`) and a `SettingsHandle { settings, path }`
that carries the settings together with the file they came from. `main` resolves the path once and
passes the handle to `run`/`App::new`; `App` holds `settings` and `settings_path`. The untyped
`load()`/`save()` wrappers are gone, so the path is resolved in exactly one place.

`crates/tui` is a binary crate whose `lib.rs` does not export `settings`, so these items are
unreachable from outside the crate and this is not a public-API change — no version bump
(Non-Negotiable Rule 6). The test constructor defaults `settings_path` to a unique temp file, making
isolation structural rather than something each test must remember.

A `dyn SettingsStore` trait seam was considered and rejected: `Settings` is one small JSON blob
behind two functions, with no second backend in prospect. Constructor injection gets the full
testability benefit without new surface area.

### 5.5 Modal lifecycle on session loss

`dismiss_modals` clears any open modal (and bumps its nonce) when the session goes away — both on a
server-driven `ws_closed` and on explicit sign-out. Without it, a modal opened while connected
floats over the sign-in screen, swallows every keypress (`handle_key` routes on
`self.models.is_some()` before the mode match), and on Esc restores the captured
`models_return = Mode::Connected` — putting the user back on the connected screen with no session.

## 8. Risks & Open Questions

- **Low — a third modal** alongside help and connect. The three share the centered-popup look; the
  shared `draw_popup` helper prevents drift. Key routing order in `handle_key` is mutually exclusive
  (only one modal is open at a time).
- **Low — pre-highlight depends on the API reporting the exact stored id.** A stored id not present in
  the fetched list falls back to index 0 (the user may re-pick or type manually).
- **Low — `/model <id>` still keys off the active provider** and is unaffected; the new command does
  not alias it, so there are two ways to set a model (intentional: one list-based, one typing-based).
- **None — semver.** All changes are TUI-internal (new enum/event/command/strings); no public
  interface, wire type, or crate boundary changes; no `Cargo.toml` bump.
- **Resolved — Ollama** is covered (issue's open question), per §2.3.
