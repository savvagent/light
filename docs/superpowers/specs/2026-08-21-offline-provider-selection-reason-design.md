# Offline provider selection reason design

> **Status:** DRAFT — surface *why* provider selection fell back to the offline `LocalProvider`,
> state it in the engine pane before the first turn, and report "no provider configured" instead
> of `invalid_plan`.

## Premise corrections

The issue's premises hold against the repository as it stands, with two clarifications recorded
here rather than built around silently:

1. **There is no settings UI yet.** The issue notes this is independent of the provider/credentials
   settings work; this change only improves the *diagnosis* of the offline fallback, not the
   mechanism for configuring a provider. Env variables remain the sole configuration surface.
2. **`LocalProvider` is only ever the offline fallback.** No env var selects it directly — it is
   reached only when selection degrades (`crates/providers/src/selection.rs:292`, `:286`). So
   "provider id is `local`" is a faithful, in-process signal for "no live provider was selected",
   but the engine currently cannot observe that signal without a seam (§3).

## Scope

**In:**

- `BuiltProvider` (selection.rs) carries `why` the offline provider was selected — a structured
  `OfflineReason` (`NothingConfigured` | `NamedProviderMissingKey` | `BaseUrlRejected`) — plus the
  selection warnings, instead of only `provider` + `model`.
- Selection stops writing warnings to stderr (`eprintln!`); it collects them on the result so the
  TUI can surface them (they are currently swallowed by the alternate screen).
- A default `Provider::is_offline()` on the engine-core seam, overridden by `LocalProvider`, so the
  engine can detect the offline fallback without depending on a string literal.
- The engine reports `no_provider_configured` for a turn against the offline provider instead of
  letting the plan parse fail as `invalid_plan`.
- The TUI states the offline status + the variables to set in the engine pane on entry, and shows
  the selection warnings there.

**Out:**

- Any settings UI, credential storage, or a way to *configure* a provider from inside the TUI
  (separate provider/credentials issue).
- Any change to the auth spine, `protocol` wire types, `server`, `persistence`, or `web/`.
- Moving provider selection server-side; selection remains in-process in the TUI.
- Replacing `eprintln!` warnings with a logging framework elsewhere in the repo — only the
  provider-selection warnings move off stderr.

## §1 `OfflineReason` and `BuiltProvider` metadata

`crates/providers/src/selection.rs` gains a public enum:

```rust
/// Why selection fell back to the offline `LocalProvider`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineReason {
    /// No provider configured: no `LIGHT_OLLAMA`, no `LIGHT_REMOTE_PROVIDER`, and none of the
    /// API keys are present.
    NothingConfigured,
    /// `LIGHT_REMOTE_PROVIDER` named a provider whose API key is absent.
    NamedProviderMissingKey { selector: String, key: String },
    /// A `*_BASE_URL` override was rejected (invalid or non-UTF-8); no provider was constructed
    /// so the API key is never sent to an unvalidated host.
    BaseUrlRejected { var: String },
}
```

`BuiltProvider` (`selection.rs:19`) becomes:

```rust
pub struct BuiltProvider {
    pub provider: Box<dyn Provider>,
    pub model: Option<String>,
    /// `Some(reason)` when the offline `LocalProvider` was selected; `None` when a live
    /// provider (Ollama or a remote) was selected.
    pub offline: Option<OfflineReason>,
    /// Human-readable selection warnings, for the TUI to surface (formerly `eprintln!`).
    pub warnings: Vec<String>,
}
```

The three reasons map to the three offline paths in `build_provider_from_env`
(`selection.rs:266`):

- **`NothingConfigured`** — `choose(false, None) => Slot::Local` (`selection.rs:292`), i.e. key
  precedence found no key and no selector was usable. Produced in `build_provider_from_env` and
  in `select_remote_from`'s key-precedence fallthrough.
- **`NamedProviderMissingKey`** — `present_or_warn` (`selection.rs:109`) returns `None` when a
  named selector's key is absent. Its `selector` field is the normalized `RemoteChoice::id()`
  (`"openai"`, etc.), not the raw `LIGHT_REMOTE_PROVIDER` value (which `present_or_warn` never
  sees); `key` is the concrete env var name (`OPENAI_API_KEY`, …).
- **`BaseUrlRejected`** — `build_remote` (`selection.rs:233`) returns `None` when `env_base_url`
  rejects a `LIGHT_*_BASE_URL` override. Produced in `build_remote`, not `select_remote_from`.

`RemoteSelection.offline` (the internal record from `select_remote_from`) therefore carries only
`NothingConfigured` / `NamedProviderMissingKey`; the `BaseUrlRejected` reason originates in
`build_remote` and is merged onto the final `BuiltProvider` by `build_provider_from_env`.

Both new members are additive fields on a public struct and a new public enum — semver-minor, no
`Cargo.toml` bump (Non-Negotiable Rule 6). `OfflineReason` is re-exported from
`crates/providers/src/lib.rs` (alongside `BuiltProvider`) so the TUI imports it from
`light_factory_providers`.

## §2 Warning collection instead of `eprintln!`

All four `eprintln!` call sites in `selection.rs` are converted to collected warnings:

1. unknown `LIGHT_REMOTE_PROVIDER` selector (`selection.rs:87`),
2. named selector whose key is absent (`selection.rs:113`),
3. `*_BASE_URL` override rejected by `validate_base_url` (`selection.rs:191`),
4. `*_BASE_URL` override that is not valid UTF-8 (`selection.rs:221`).

The pure helpers are already side-effect-light; the warnings are threaded through as return values
rather than printed, so the pure precedence table stays unit-testable without `set_var`:

- `select_remote_from` (`selection.rs:69`) returns a small internal `RemoteSelection
  { choice: Option<RemoteChoice>, offline: Option<OfflineReason>, warnings: Vec<String> }`
  (no longer `Option<RemoteChoice>`).
- `present_or_warn` returns a `RemoteSelection` carrying `NamedProviderMissingKey` and its warning
  when the key is absent.
- `resolve_base_url` (`selection.rs:180`) returns `Result<String, BaseUrlRejection>` where
  `BaseUrlRejection { var, warning }` replaces the printed rejection. `env_base_url`
  (`selection.rs:212`) maps a non-UTF-8 override to the same shape.
- `build_remote` returns the provider plus the `BaseUrlRejected` reason and warning.

The warning text itself is unchanged — only the sink moves from stderr to the result — so the
messages remain recognizable to anyone who saw them on stderr before. `build_provider_from_env`
aggregates all warnings onto `BuiltProvider.warnings`.

## §3 `is_offline` seam and the engine guard

`crates/engine-core/src/traits.rs` (`Provider` trait, `traits.rs:10`) gains a default method:

```rust
/// Whether this provider is the offline fallback (performs no real completion). The engine
/// uses this to reject a turn up front rather than failing later on plan parsing.
fn is_offline(&self) -> bool { false }
```

`LocalProvider` (`crates/providers/src/local.rs:25`) overrides it to `true`. `ScriptedProvider`
keeps the default (`false`) — it is a test/demo provider that returns valid structured output, not
the offline fallback, so engine tests keep exercising the real plan-parse path.

Adding a default method to a public trait is additive (semver-minor) — no version bump.

`crates/engine/src/turn.rs::run_turn` (`turn.rs:49`) guards at the top, before `propose_plan`:

```rust
if self.provider.is_offline() {
    self.emit(EventKind::Error {
        code: "no_provider_configured".into(),
        message: "no provider configured — set an API key or LIGHT_OLLAMA=1".into(),
    });
    self.emit(EventKind::TurnComplete { ok: false });
    return;
}
```

This replaces the current failure mode, where `propose_plan` (`turn.rs:80`) parses the
`LocalProvider` echo as a `Plan`, fails, and emits `invalid_plan` (`turn.rs:105`). The guard emits
one clear error and ends the turn without ever calling the provider.

## §4 TUI surfacing + i18n

`crates/tui/src/provider.rs`:

- `ProviderInfo` (`provider.rs:10`) gains `offline: Option<OfflineReason>` and
  `warnings: Vec<String>`, populated from the `BuiltProvider`.
- A pure `offline_notice(locale, &OfflineReason) -> String` helper maps the three reasons to the
  i18n catalog so the mapping is unit-testable (mirrors `engine_view::describe_event`).

`crates/tui/src/app.rs::enter_engine` (`app.rs:243`), which currently rebuilds the provider and
discards its info (`let (provider, _info) = crate::provider::build()`), now:

1. pushes every selection warning into `engine_log` (visible in the engine pane on entry), then
2. when `info.offline` is `Some(reason)`, pushes `offline_notice(lang, reason)` naming the
   variable(s) to set.

The warnings/notice are pushed **after** the existing `self.engine_log.clear()` (`app.rs:271`) —
which currently runs near the end of `enter_engine` — so the clear does not wipe the notice.

`crates/tui/src/i18n.rs` gains EN + ES entries (ES must mirror EN exactly — test-enforced):

- `provider.offline.nothing` — "No provider configured — set ANTHROPIC_API_KEY (or another
  provider's key) or LIGHT_OLLAMA=1"
- `provider.offline.missing_key` — "Provider '{selector}' selected but {key} is not set — falling
  back to offline"
- `provider.offline.base_url` — "{var} was rejected — falling back to offline"
- `error.no_provider_configured` — EN: "No provider configured — set an API key or LIGHT_OLLAMA=1";
  ES: "No hay proveedor configurado — define una clave de API o LIGHT_OLLAMA=1". Translated by
  `describe_event` via `error_message(locale, code)` (`engine_view.rs:79`) when the engine emits
  the guard error; the engine's `message` field is only the fallback for a catalog miss.

## Assumptions

1. **The engine guard is a boolean (`is_offline`), the detailed reason lives in the TUI.** The
   engine only needs to distinguish "offline fallback" from "live provider"; the structured
   `OfflineReason` (which variables to set) is a display concern owned by the TUI, which already
   has the `BuiltProvider`. This avoids widening `Engine::new` (a breaking signature change) or
   coupling the engine to the `providers` crate's selection types.
2. **`is_offline()` returns `true` only for `LocalProvider`.** Rationale: it is the sole offline
   fallback; `ScriptedProvider` must stay `false` so existing engine tests (which drive real plan
   parsing with canned JSON) are unaffected.
3. **Warnings move fully off stderr for provider selection.** Rationale: the TUI is the only
   consumer today and the alternate screen swallows stderr; keeping a copy on stderr would print
   during startup (before the alternate screen) and duplicate the in-pane message.
4. **Engine-mode entry is where the notice appears.** Rationale: the AC targets the engine pane
   ("entering engine mode … states that plainly in the engine pane"). The `/ask` path in the
   connected screen is unchanged (the offline provider still answers deterministically there).
5. **No version bump.** All public-surface changes are additive (new enum, new struct fields, new
   default trait method).

## Goal & Success Criteria

Goal: a user who starts the TUI with no provider configured is told, in the engine pane and before
any turn runs, that no provider is configured and which variables to set — instead of getting a
misleading `invalid_plan` on their first turn.

- [ ] `BuiltProvider` carries `offline: Option<OfflineReason>` (three reasons) and the collected
      warnings, with no `eprintln!` left in `selection.rs`.
- [ ] Entering engine mode with no configured provider states it plainly in the engine pane and
      names the variables to set.
- [ ] A turn against the offline provider emits `Error { code: "no_provider_configured" }` and
      `TurnComplete { ok: false }`, never `invalid_plan`.
- [ ] Selection warnings (named-but-missing key, rejected base URL, unknown selector) appear in the
      TUI engine pane.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
      `cargo fmt --all --check` are clean; ES mirrors EN.

## Error Handling & Edge Cases

- Nothing configured → `NothingConfigured`, no warnings; the pane shows the "nothing configured"
  notice naming the variables.
- `LIGHT_REMOTE_PROVIDER=openai` with no `OPENAI_API_KEY` → `NamedProviderMissingKey` + warning;
  still offline (never misroutes to a different key).
- `LIGHT_OPENAI_BASE_URL` invalid/non-UTF-8 → `BaseUrlRejected` + warning; offline, key never sent.
- Unknown `LIGHT_REMOTE_PROVIDER` with a valid key present → a *live* provider (`offline: None`)
  plus a warning about the unknown selector; the notice is not shown.
- A live provider selected → `offline: None`, `warnings` possibly non-empty (unknown selector).
- `ScriptedProvider`/`LocalProvider` in engine tests → `is_offline()` false/true respectively, so
  existing tests and the new offline test both behave.

## Risks & Open Questions

- **Low — `is_offline` default-method growth.** Adding a default method to the engine-core trait is
  the smallest possible seam. If a future provider legitimately needs an offline-but-real mode, the
  boolean can evolve; for this change a bool is exactly sufficient.
- **Low — warnings surfacing point.** The engine pane is the only surfacing point today. If a
  settings screen is added later, it should reuse `BuiltProvider.warnings`/`offline` rather than a
  second warning mechanism.
- **None — semver.** All changes are additive; the semver-minor convention holds.
