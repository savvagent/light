# Provider selection & credential supply in the client — design

> **Status:** DRAFT — add an in-client surface to see, select, and switch the LLM provider and to
> supply a credential that persists across runs, while keeping env-driven selection and headless
> behavior unchanged.

> **Implements:** GitHub issue savvagent/light-factory#23

## Background

Provider selection today is entirely environment-driven and read once per process. There is no way
to see available providers, switch without restarting, or supply an API key from inside the client.
The selection logic in `crates/providers/src/selection.rs` is sound and well-tested; the surface
above it is missing.

The issue requires a written design before implementation, because where a credential lives on disk
is a security decision. The four open questions it poses — storage, precedence, global-vs-per-session,
and redaction — are answered in §1–§4. The fifth ("mid-session switch allowed?") is answered in §3.

## Premise corrections

None. The issue's description matches the repository as of `850c918`. Two clarifications recorded so
they are not built around silently:

1. **The provider is constructed *twice* today, not once.** `crates/tui/src/main.rs:50` builds it at
   startup for the header and `/ask`; `crates/tui/src/app.rs:246` rebuilds it inside `enter_engine`
   for the engine. This change collapses both onto a single source of truth held by `App` (§5), which
   is itself a fix for the "changing provider means restarting" symptom.
2. **`Settings` persists exactly one field (`lang`)** (`crates/tui/src/settings.rs:10`), and the
   `SecretCipher` in `crates/auth/src/secret.rs` is keyed by the *server-side* `LIGHT_SECRET_KEY`,
   which the TUI client does not and must not have. So the encrypted-file option from the issue is not
   directly reusable: it would require a *second* client-local key whose own storage re-raises the
   same problem. This is the primary reason the OS keyring is chosen (§1).

## Scope

**In:**

- A way to see the available providers, which is active, and *why* it was selected (§6).
- In-session selection of a provider and model, persisted across runs as non-secret preferences, with
  no client restart (§3, §5).
- A masked `/key` command to supply or clear a credential, persisted in the OS keyring (§1, §4).
- Documented, testable precedence between env, persisted preferences, and the keyring (§2).
- Redaction of credentials everywhere they could otherwise appear (§4).

**Out:**

- Persisting a per-provider `*_BASE_URL` override. Base URLs remain env-only
  (`LIGHT_OPENAI_BASE_URL` / `LIGHT_DEEPSEEK_BASE_URL`); the validation trust boundary is unchanged.
- Any change to the auth spine, `protocol` wire types, `server`, `persistence`, or `web/`.
- Moving provider selection server-side; selection stays in-process in the TUI.
- Mid-*turn* provider switching inside an engine session (§3).
- Any fallback that writes a credential to a plaintext file (explicitly rejected, §1).

## §1 Storage: OS keyring, never a plaintext file

Credentials are stored in the **OS keyring** via the `keyring` crate, one entry per provider:
service `"light-factory"`, account `"<provider-id>"` (e.g. `"openai"`). The OS keyring is the
correct home for a per-user secret on a client: Linux Secret Service / macOS Keychain / Windows
Credential Manager all encrypt at rest and are scoped to the user, which a plaintext or
keyless-encrypted file cannot provide.

Two decisions follow from this:

- **No plaintext, and no encrypted-file fallback.** A plaintext file violates the issue's redaction
  requirement outright. An encrypted file would need a client-local key; the only keyed primitive in
  the repo (`SecretCipher`, `crates/auth/src/secret.rs:19`) is keyed by the *server-side*
  `LIGHT_SECRET_KEY`, which must never ship to clients, so a client-side encrypted file would have to
  mint and store a second key — re-raising the chicken-and-egg problem the keyring already solves.
- **Env is the headless/CI fallback, not a degraded client store.** Where the OS keyring is
  unavailable (headless server, CI, no Secret Service), `/key` reports the failure and points the
  operator at the existing env variables. Selection itself never requires the keyring.

The keyring is accessed behind a small `CredentialStore` trait (`crates/tui/src/credentials.rs`) with
a real `KeyringStore` impl and an in-memory impl for tests, so the rest of the client is unit-tested
without a live keyring. `crates/providers` does **not** gain a `keyring` dependency: it stays a leaf,
and the client injects resolved keys (§5).

## §2 Precedence (env wins; documented and test-enforced)

A provider's key is *resolved* as: `env(<P>_API_KEY)` if non-empty, else the keyring entry for `<P>`,
else absent. **Environment always wins over the keyring for the same provider**, so a CI/headless run
that exports `OPENAI_API_KEY` behaves exactly as it does today even if a stale key sits in the keyring.

Selection order (each step returns when it can name a provider, and records *why* — §6):

1. `LIGHT_OLLAMA=1` → Ollama (`SelectedBy::OllamaEnv`).
2. `LIGHT_REMOTE_PROVIDER` naming a valid provider → that provider (`SelectedBy::RemoteSelectorEnv`),
   key resolved env→keyring. A valid name with **no** key → offline `NamedProviderMissingKey`
   (unchanged: never misroute to a different keyed provider).
3. The persisted `provider` preference (`config.json`) → that provider (`SelectedBy::StoredPreference`),
   key resolved env→keyring. A stored remote with no key → offline `NamedProviderMissingKey`, for the
   same never-misroute reason as step 2. A stored `"ollama"` selects the Ollama slot directly.
4. Key precedence `Anthropic > OpenAI > Gemini > DeepSeek`, where "present" means env **or** keyring
   (`SelectedBy::KeyPrecedence`).
5. Offline `LocalProvider` (`offline: Some(…)`, `selected_by: None`).

Model resolution: `LIGHT_<P>_MODEL` env > persisted `models[P]` > the provider's default constant.
An env `LIGHT_<P>_MODEL` therefore still overrides a `/model` preference — the same env-wins rule.

The existing `build_provider_from_env()` is preserved as a thin wrapper: it builds a `Selection`
from the environment (empty keyring, empty preference) and calls the new injectable
`build_provider(&Selection)`, so today's env-only behavior and its unit tests are untouched. All
precedence rules above are pure functions of an injected `Selection`, unit-testable without touching
process env or a keyring.

## §3 Global vs per-session; when a switch takes effect

Provider choice is **global per client process**, matching how `Engine` holds one
`Arc<dyn Provider>` shared by every session (`crates/engine/src/lib.rs:22`, cloned at
`create_session` → `Session::spawn`). The client holds the single active provider on `App`
(`crates/tui/src/app.rs:90`) and rebuilds it in place when a `/provider`, `/model`, or `/key` command
changes the selection.

A switch takes effect **immediately** for `/ask` and for the header, and on the **next engine
session**. Mid-*turn* switching is explicitly out of scope: the engine session is a long-lived actor
with a pinned provider, and switching mid-turn would require tearing down the running session. To
switch the engine's provider the user leaves engine mode, runs `/provider …`, and re-enters — no
client restart. This is the answer to the issue's "allowed at all, or only between sessions"
question: between sessions only.

## §4 Redaction

- Credentials never reach `config.json` (only non-secret preference ids live there, §5).
- Credentials never reach the engine: the engine only ever sees the constructed `Arc<dyn Provider>`,
  and providers (`crates/providers/src/{openai,anthropic,gemini,deepseek,ollama}.rs`) do not expose or
  echo their key; their HTTP clients already redact secrets from errors (the `base_url` module's
  redaction guarantees, inherited from the port).
- Credentials never reach `engine_log`, the activity `log`, `EventKind::Error`, the transcript, or
  any status line: `/key` status messages say only "set" / "cleared" / "failed", never the value.
- Key input is **masked**: the `/key` entry mode renders placeholder characters, never the typed key,
  and the buffer is dropped on cancel.

## §5 Client surface

`crates/tui/src/selection.rs` (new module) owns the client-side composition, and **takes over the
`build()` composition role** from `crates/tui/src/provider.rs`. There is exactly one composition site:

- `resolve_key(provider_id, store)` → env then keyring (the §2 rule).
- `key_status(provider_id, store) -> KeyStatus { Env, Keyring, None }` → which source, if any, holds a
  key for a provider; used by the bare `/key` enumeration (§6), distinct from `resolve_key`.
- `build_selection(prefs, store)` → assembles a `light_factory_providers::Selection` from env +
  persisted preferences + the keyring, applying the §2 precedence.
- `rebuild(prefs, store)` → `build_provider(&Selection)` plus an enriched
  `ProviderInfo { id, model, offline, selected_by, warnings }` for display (§6).

`crates/tui/src/provider.rs` is retained but **only** for the display record — `ProviderInfo` (now
with `selected_by`) and the `offline_notice`/reason-rendering helpers — its `build()` is removed, so
no second composition path exists. `main.rs` and `App` call `selection::rebuild`, never
`provider::build`.

`crates/providers/src/selection.rs` gains (all additive, semver-minor — Non-Negotiable Rule 6):

- `pub struct Selection` — the explicit resolved inputs (ollama flag, selector, preferred id, resolved
  keys, model overrides, base-url overrides). Internal `RemoteChoice`/`RemoteSelection`/`build_remote`
  are threaded to read from it instead of `std::env`.
- `pub fn build_provider(&Selection) -> BuiltProvider` — the injectable entry point.
- `pub fn build_provider_from_env() -> BuiltProvider` — the unchanged wrapper.
- `pub enum SelectedBy { OllamaEnv, RemoteSelectorEnv, StoredPreference, KeyPrecedence }` and a
  `BuiltProvider.selected_by: Option<SelectedBy>` (`None` ⇔ offline), so the TUI can state *why* a
  provider is active.
- A `pub fn resolve_key(provider_id, keyring: &dyn Fn(&str) -> Option<String>)` helper is **not**
  added to `providers` — key resolution stays in the client, so `providers` remains env-agnostic except
  for the wrapper. (The client passes already-resolved keys into `Selection`.)

`crates/tui/src/settings.rs` extends the persisted `Settings` with non-secret fields only:

```rust
pub struct Settings {
    pub lang: String,
    #[serde(default)] pub provider: Option<String>,        // "anthropic"|"openai"|"gemini"|"deepseek"|"ollama"
    #[serde(default)] pub models: BTreeMap<String, String>, // per-provider model override
}
```

Old files with only `lang` still parse (serde `default`). Existing `load_lang`/`save_lang` are
replaced by load/save of the whole struct; the `/lang` call site is updated to preserve its behavior.

`crates/tui/src/app.rs` gains the commands (parsed in `run_command`), each a pure helper where the
argument needs parsing:

- `/provider` — list available providers, which is active, and why (appends lines to the log).
- `/provider <name>` — select a provider (`anthropic|openai|gemini|deepseek|ollama`); persists the
  preference and rebuilds in place.
- `/model <id>` — set the model for the active provider; persists and rebuilds.
- `/key` — show which providers have a key and from where (env vs keyring), never the value.
- `/key <provider>` — enter masked entry mode to set/update the credential (keyring).
- `/key <provider> clear` — remove the stored credential.

`enter_engine` (`crates/tui/src/app.rs:245`) stops rebuilding from env and instead clones
`self.provider` / `self.provider_info`, collapsing the double-build and honoring in-session switches.

## §6 Seeing "which one is active and why"

The connected header (`info.connected`) already shows the provider id + model. It gains a short
reason suffix derived from `selected_by` / `offline` (e.g. "env LIGHT_REMOTE_PROVIDER", "stored
preference", "key precedence", "offline: <reason>"). When selection reached an offline fallback with
a *known attempted selection* (steps 2–3 of §2), the reason renders the full
`OfflineReason::NamedProviderMissingKey { selector, .. }`, so the user sees **which** provider they
named and **which** key variable is missing — the attempted selection is not dropped to a bare
"offline". `/provider` prints the full list with each provider's key status via `key_status`
("env" / "keyring" / "none") and marks the active one. New EN + ES i18n keys are added for every new
user-facing string; ES mirrors EN exactly (test-enforced in `i18n.rs`).

## Assumptions

1. **OS keyring is available on developer workstations; it is not required anywhere else.** Rationale:
   the TUI is a developer laptop client; CI/headless already use env. A missing keyring degrades to a
   clear `/key` error that points at env, never to a plaintext fallback.
2. **Env always beats persisted preference and keyring for the same provider.** Rationale: it keeps
   CI/headless runs byte-for-byte unchanged and is the standard Unix env-over-config convention the
   issue asks be "predictable".
3. **A stored preference with a missing key behaves like `LIGHT_REMOTE_PROVIDER` with a missing key —
   offline, not silent misrouting.** Rationale: this preserves the existing `present_or_warn` guard's
   intent; surprising fallthrough to another provider is exactly what that guard exists to prevent.
4. **Model is persisted per provider, not globally.** Rationale: `LIGHT_<P>_MODEL` is per provider;
   a single global override would be wrong the moment the user switches providers.
5. **No per-provider base URL persistence.** Rationale: base URLs are a trust boundary with
   validation semantics (loopback-only `http`, secret redaction); expanding their persistence is a
   separate change and out of scope here.
6. **`SelectedBy`/`Selection` are additive public surface, no version bump.** Rationale: new enum, new
   struct, new struct field, and a new free function are all semver-minor by this repo's convention.

## Goal & Success Criteria

Goal: a user can, from inside the TUI and without restarting, see the available providers, see which
is active and why, switch provider and model, and supply a credential that persists across runs —
with the credential masked on input, redacted everywhere, and env/CI behavior unchanged.

- [ ] `/provider` shows available providers, the active one, and why; `/provider <name>` switches
      without a client restart (affects `/ask` immediately and the next engine session).
- [ ] `/key <provider>` accepts a masked credential, persists it to the OS keyring, and redacts it in
      every log/status/transcript path; `/key <provider> clear` removes it.
- [ ] `/model <id>` sets and persists a per-provider model override.
- [ ] Env selection (incl. `LIGHT_OLLAMA`, `LIGHT_REMOTE_PROVIDER`, `*_API_KEY`, `LIGHT_<P>_MODEL`)
      behaves unchanged, and env wins over persisted/keyring values — proven by unit tests.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`,
      `cargo fmt --all --check` clean; ES mirrors EN; no plaintext credential on disk.

## Error Handling & Edge Cases

- `/provider <unknown>` → localized "unknown provider" error; selection unchanged.
- `/provider <name>` with no key available → the switch succeeds and the header/log shows the
  offline reason (`NamedProviderMissingKey`) rather than misrouting to another key.
- `/key <provider>` when the OS keyring is unavailable → a localized error naming the env variables
  to set instead; nothing is written, the typed buffer is discarded.
- `/key` with a blank entry → no-op with a hint, no keyring write, no empty credential stored.
- `config.json` missing/corrupt or an older `{ "lang": … }`-only file → default preferences, still
  parseable (serde `default`), never a startup error.
- A completion or engine turn uses the provider captured at the time it started; a switch that
  happens concurrently does not retroactively affect an in-flight request.

## Risks & Open Questions

- **Medium — `keyring` Linux backend requires a running Secret Service.** On a headless box `/key`
  reports failure and points to env; acceptable because that is already the documented headless path,
  but it means the "persists across runs" AC is met only where a keyring exists. Recorded as a
  deliberate trade, not a gap.
- **Medium — `keyring` Linux backend may also require `libdbus`/`pkg-config` at *build* time.** The
  plan's first task must verify `cargo build -p light-factory-tui` (and `cargo test --workspace`)
  succeeds on this machine and in CI before any feature code lands; if the sync Secret-Service backend
  does not build cleanly, the `CredentialStore` trait is the seam where a lighter backend is swapped in
  without touching selection or the TUI.
- **Low — dependency weight.** `keyring` pulls a D-Bus/Secret-Service stack on Linux. Isolated behind
  the `CredentialStore` trait; if the build proves heavy, the trait is the seam where an alternative
  backend would swap in without touching selection or the TUI.
- **Low — `SelectedBy` naming.** If a future milestone moves selection server-side, `SelectedBy`
  should move with it; it is additive and re-exported, so that migration is non-breaking.
- **None — semver.** All public-surface changes are additive.
