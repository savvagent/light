# Port otto's LLM providers into light-factory design

> **Status:** IMPLEMENTED (2026-08-20) — port the `otto-providers` crate (OpenAI / Anthropic /
> DeepSeek / Gemini / Ollama / Local / Scripted plus the `base_url` trust-boundary module) into a
> new `crates/providers` crate and wire an env-driven provider selection into the TUI with a
> `/ask` completion command.

## Premise corrections

The task brief — "Port the providers from /home/robhicks/dev/otto to light-factory and wire them
into the TUI" — survives contact with both repositories, with three corrections that shape the
design:

1. **Otto keeps providers in-process behind a trait, selected by env.** In otto, providers are not
   wired into a TUI/CLI at all: they live in `otto-providers` (an in-process library behind
   `otto_engine_core::traits::Provider`) and the *engine* (`crates/engine/src/lib.rs`,
   `build_router`) selects one from the environment (`OTTO_REMOTE_PROVIDER`, `*_API_KEY`,
   `*_MODEL`, `*_BASE_URL`, `OTTO_OLLAMA`). The user's answer to the architecture question —
   "How were they implemented in Otto?" — selects this model: an in-process `Provider` trait
   selected by env, not a new server endpoint and not a provider-agnostic "port the crate only".
   light-factory has no engine crate yet, so the selection logic lives in the providers crate as a
   reusable `build_provider_from_env()` and the TUI is its first consumer.
2. **light-factory now has an `engine-core`.** Otto's `Provider` trait and `CompleteRequest` /
   `CompleteResponse` / `Usage` types live in `otto-engine-core`. An earlier revision of this spec
   folded them into `crates/providers`, on the grounds that three small types did not justify a
   crate before the engine existed — and recorded that "when the engine lands, these types migrate
   to it." The engine has since been designed
   (`docs/superpowers/specs/2026-08-20-engine-core-design.md`), which creates
   `crates/engine-core` for exactly these seams alongside `Tool`, `Workspace`, and
   `PermissionGate`. This port therefore places the trait and types in `crates/engine-core` and
   `crates/providers` depends on it, matching otto's layout. Recorded in Assumptions §3.
3. **`candle` is out of scope.** `CandleProvider` runs a quantized Gemma GGUF in-process behind
   the `candle` feature, with `candle-core`/`candle-transformers`/`tokenizers`/`hf-hub`
   dependencies and a model download at load time. It is opt-in in otto (the default build has no
   candle), brings a heavy dependency tree, and fits light-factory's server/engine milestone
   better than a client TUI. Porting it is deferred (Risks §2).

## Scope

**In:**

- New `crates/providers` crate (a leaf: no light-factory crate dependencies), porting from
  `otto-providers`:
  - `Provider` trait (`id()`, `complete(CompleteRequest) -> CompleteResponse`) and the
    `CompleteRequest` / `CompleteResponse` / `Usage` types (from `otto-engine-core`) — these land
    in `crates/engine-core`, not `crates/providers`.
  - `OpenAiProvider`, `AnthropicProvider`, `DeepSeekProvider`, `GeminiProvider`,
    `OllamaProvider`, `LocalProvider`, `ScriptedProvider`.
  - The `base_url` trust-boundary module (`validate_base_url`, `BaseUrlError`,
    `build_http_client`, `reject_redirect`, `join_url`, loopback-host checks) and the shared
    `openai_compatible` wire implementation.
  - A `build_provider_from_env()` selection entry point (port of otto's `build_router` +
    `preflight_base_urls`, simplified to a single slot — no brain-blend/pinned router) plus the
    pure, injectable selection helpers so they are unit-testable without touching process env.
- TUI wiring: the TUI constructs a provider at startup, shows the active provider in the connected
  header, and adds a `/ask <prompt>` command that runs a completion against it and appends the
  result to the activity log. Falls back to the offline `LocalProvider` when no provider is
  configured (mirroring otto's offline fallback).

**Out:**

- `CandleProvider` and the `candle` feature (deferred; Risks §2).
- Any server-side completion endpoint or WebSocket `Command`/`Event` change. Providers run
  in-process in the TUI for now; moving them server-side is the engine milestone, not this task.
- Any change to the auth spine, `protocol` wire types, `server`, `persistence`, or `web/`.
- Streaming, tool/function calling, and multi-turn conversation history — the ported `complete`
  surface is single-shot prompt → text, exactly as in otto.

## §1 Provider trait and types

Ported verbatim into `crates/engine-core` — see Assumptions §3:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    async fn complete(&self, req: CompleteRequest) -> anyhow::Result<CompleteResponse>;
}

pub struct CompleteRequest { pub prompt: String }
pub struct CompleteResponse { pub text: String, pub usage: Option<Usage> }
pub struct Usage { pub input_tokens: u32, pub output_tokens: u32 }
```

Identical to otto: `id()` returns a stable provider id (`"openai"`, `"anthropic"`, `"deepseek"`,
`"gemini"`, `"ollama"`, `"local"`, `"scripted"`); `usage` is `None` for the offline/deterministic
providers. No serde derives are added — these are in-process types, not wire types, so no
semver/protocol constraint applies (Non-Negotiable Rule 6 is untouched).

## §2 Provider implementations

Each provider is a faithful port with only mechanical renames: `otto_engine_core::traits::Provider`
→ `light_factory_engine_core::Provider`, `otto_engine_core::types::{…}` →
`light_factory_engine_core::{…}`, and `crate::base_url` in place
of `super::base_url`. Wire shapes, headers, token-field policy, error surfacing, redirect guards,
and tests are unchanged.

| Provider | Endpoint | Auth | Notes |
|---|---|---|---|
| `OpenAiProvider` | `{base}/v1/chat/completions` | `Authorization: Bearer` | o-series uses `max_completion_tokens`; else `max_tokens` |
| `AnthropicProvider` | `{base}/v1/messages` | `x-api-key` + `anthropic-version` | — |
| `DeepSeekProvider` | `{base}/chat/completions` | `Authorization: Bearer` | shares `openai_compatible`, always `max_tokens` |
| `GeminiProvider` | `{base}/v1beta/models/{model}:generateContent` | `x-goog-api-key` | — |
| `OllamaProvider` | `{base}/api/generate` | none (local) | `local_default(model)` → `http://127.0.0.1:11434` |
| `LocalProvider` | n/a (offline) | none | deterministic transform of the prompt |
| `ScriptedProvider` | n/a (offline) | none | canned responses keyed by prompt substring |

Security properties inherited from otto (and preserved by construction): every HTTP provider uses
`build_http_client` (redirects disabled; system proxy off for `http` bases) and calls
`reject_redirect` before parsing; `validate_base_url` accepts `https` always and `http` only to
loopback, rejects userinfo/query/fragment, and redacts secrets from error messages. The
`x-api-key` / `x-goog-api-key` / `Authorization` headers are never forwarded on redirect because
redirects are disabled at the client.

## §3 Provider selection (`build_provider_from_env`)

Ported from otto's `build_router`/`select_remote_from`/`preflight_base_urls`, simplified to a
single slot because light-factory has no router yet. Env contract (otto names → light names):

| Otto var | light-factory var | Meaning |
|---|---|---|
| `OTTO_REMOTE_PROVIDER` | `LIGHT_REMOTE_PROVIDER` | `anthropic\|openai\|gemini\|deepseek` — named remote wins when its key is present |
| `ANTHROPIC_API_KEY` | `ANTHROPIC_API_KEY` (unchanged) | key presence selects Anthropic |
| `OPENAI_API_KEY` | `OPENAI_API_KEY` (unchanged) | key presence selects OpenAI |
| `GEMINI_API_KEY` | `GEMINI_API_KEY` (unchanged) | key presence selects Gemini |
| `DEEPSEEK_API_KEY` | `DEEPSEEK_API_KEY` (unchanged) | key presence selects DeepSeek |
| `OTTO_*_MODEL` | `LIGHT_*_MODEL` | per-provider model override |
| `OTTO_OLLAMA` | `LIGHT_OLLAMA` | `1` selects Ollama (local slot) |
| `OTTO_OLLAMA_MODEL` | `LIGHT_OLLAMA_MODEL` | Ollama model id |
| `OPENAI_BASE_URL` | `LIGHT_OPENAI_BASE_URL` | validated base URL override |
| `DEEPSEEK_BASE_URL` | `LIGHT_DEEPSEEK_BASE_URL` | validated base URL override |

Selection order, matching otto's `select_remote_from` exactly: `LIGHT_OLLAMA=1` →
`OllamaProvider`; else `LIGHT_REMOTE_PROVIDER` (a *valid name whose key is present*) → that
provider; else key precedence `Anthropic > OpenAI > Gemini > DeepSeek` (first key present wins);
else `LocalProvider` (offline fallback, never an error). The degradation rules mirror otto's
`present_or_warn` precisely:

- A named selector whose key is **absent** → warn and select **nothing** (offline `LocalProvider`),
  never fall through to a *different* keyed provider — routing to another provider silently is
  exactly what otto's `present_or_warn` prevents.
- An **unknown** selector (`LIGHT_REMOTE_PROVIDER=foo`) → warn and fall through to key precedence.
- A `*_BASE_URL` override rejected by `validate_base_url` → warn and construct no provider for that
  slot (offline), so the API key is never sent to an unvalidated host.

`validate_base_url` runs on any `*_BASE_URL` override before a provider is constructed.

Default models are the otto constants: `claude-haiku-4-5`, `gpt-4o-mini`, `gemini-2.5-flash`,
`deepseek-v4-flash`, `llama3.2`.

## §4 TUI wiring

- `crates/tui/src/provider.rs`: `build()` that calls `light_factory_providers::build_provider_from_env()`
  and returns a `Box<dyn Provider>` plus a small `ProviderInfo { id: String, model: Option<String> }`
  for the header display. Construction is fail-closed: selection never errors (it always yields at
  least `LocalProvider`); any `*_BASE_URL` rejection is already handled inside the selection.
- `crates/tui/src/app.rs`: `App` gains a `provider: Arc<dyn Provider>` field (constructed once at
  `run()`), the connected header shows `· provider: <id>[ (<model>)]`, and the command handler
  adds `/ask <prompt>`: it spawns a task that awaits `provider.complete(CompleteRequest { prompt })`,
  sends the text (or the error) back to the UI loop as a new `UiEvent::Completion` variant, and the
  loop appends the result to the activity log. Completion runs off the UI loop so a slow remote
  provider never blocks input.
- `/ask` is guarded to the `Connected` mode and requires a non-empty prompt (empty → a hint, no
  call). Errors are appended to the log as `[ask] <error>` rather than dumped to the `error` field,
  so a failed completion does not bounce the user out of the connected screen.
- Prerequisite: the `/` keybinding that opens command mode is currently matched only for
  `SignIn | Register | RegisterCode` (`crates/tui/src/app.rs:164`); it must be extended to
  `Mode::Connected` for `/ask` to be reachable.

## §5 Dependencies

- New workspace dependency `async-trait = "0.1"` (otto uses it; native async-fn-in-trait with
  `dyn Provider` adds Send-bound friction for no benefit here). `async-trait` is already a direct
  dependency of `auth` and `persistence` at `0.1`; promoting it to a workspace dependency (and
  pointing those two at `{ workspace = true }`) is an incidental, behavior-neutral tidy-up folded
  into this change so all crates resolve one version.
- `crates/providers/Cargo.toml`: `anyhow`, `async-trait`, `reqwest` (workspace), `serde`,
  `serde_json`; dev-deps `tokio` (macros/rt-multi-thread), `wiremock`, `tempfile` (the latter two
  direct dev-deps, not workspace, since only this crate uses them).
- `crates/tui/Cargo.toml`: add `light-factory-providers = { path = "../providers" }` (`reqwest`
  and `futures-util` are already TUI dependencies; `async-trait` is not needed in the TUI).

Dependency flow: `providers` is a new leaf (depends on nothing but third-party crates); `tui`
gains an edge to `providers`, exactly as it already consumes the leaf `protocol`. No existing
crate gains an inward/outward edge that violates the documented flow (protocol → auth →
persistence → server → tui).

## §6 Testing

- The providers crate carries over otto's full unit-test suite (wiremock-based): request shape,
  auth headers, usage parsing, HTTP error surfacing, redirect non-following, trailing-slash base
  URLs, empty-choices → empty text, `LocalProvider` determinism, `ScriptedProvider` rule/default
  matching, and the entire `base_url` validation matrix (loopback carve-out, secret redaction,
  userinfo/query/fragment rejection, normalization, `join_url` single-separator).
- Selection helpers are pure and injectable (otto's `select_remote_from` pattern): tests assert
  the precedence table, the named-but-no-key degradation, unknown-selector fallthrough, and
  `*_BASE_URL` rejection → offline fallback, without touching process env.
- TUI: the `/ask` command parsing and empty-prompt guard are extracted as a small pure function
  with unit tests; the async completion path is exercised by a manual `cargo run -p
  light-factory-tui` smoke (offline `LocalProvider`, since no keys are set in CI/dev).

## Assumptions

1. **Wiring is in-process in the TUI, not a server endpoint.** Rationale: the user's answer —
   "How were they implemented in Otto?" — points at otto's in-process model, and light-factory's
   engine (the natural server-side home) does not exist yet. Moving providers server-side is the
   engine milestone; the ported selection API (`build_provider_from_env`) is designed so the
   server can reuse it verbatim then.
2. **Env names: light-specific vars get the `LIGHT_` prefix; the four API-key vars stay
   unprefixed.** Rationale: `LIGHT_` matches this repo's convention (`LIGHT_API_URL`,
   `LIGHT_SECRET_KEY`); `OPENAI_API_KEY`/`ANTHROPIC_API_KEY`/`GEMINI_API_KEY`/`DEEPSEEK_API_KEY`
   are industry-standard and shared with other tools — this also mirrors otto (keys unprefixed,
   selection vars prefixed).
3. **Trait + types live in `crates/engine-core`, matching otto.** Rationale: this supersedes an
   earlier assumption that folded them into `crates/providers` because no engine crate existed.
   The engine core is now designed and creates `engine-core` regardless — it must house `Tool`,
   `Workspace`, and `PermissionGate`, which plainly do not belong in a providers crate. Putting
   the provider seam anywhere else would either duplicate it or invert the dependency flow the
   architecture commits to (`protocol ← engine-core ← providers`). Executing that plan's Task 4
   before this one's Task 1 is therefore a prerequisite, recorded in Task Order below.
4. **`candle` is omitted.** Rationale: opt-in in otto, heavy dependency tree, model download at
   load time, and it targets server/engine local inference rather than a client TUI. Ported
   separately if needed (Risks §2).
5. **Single-shot completions only.** Rationale: the ported `Provider::complete` is prompt→text;
   streaming/tools/multi-turn history are otto's agents'/router's concern and are not part of the
   providers crate.

## Goal & Success Criteria

Goal: light-factory gains a faithful, security-preserving port of otto's LLM providers and the TUI
can select one from the environment and issue a completion.

- [ ] `crates/providers` compiles and its full test suite is green (`cargo test -p light-factory-providers`).
- [ ] The ported `base_url` security behavior is intact: redirects disabled, `http` loopback-only,
      userinfo/query/fragment rejected, secrets redacted in errors.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, and
      `cargo fmt --all --check` are clean.
- [ ] `cargo run -p light-factory-tui` shows the active provider and `/ask <prompt>` returns an
      offline `LocalProvider` completion when no keys are set.
- [ ] No change to `protocol`, `auth`, `persistence`, `server`, or `web/`; no wire-type or public
      API break (no version bump required).

## Error Handling & Edge Cases

- No provider configured (no keys, no `LIGHT_OLLAMA`, no `LIGHT_REMOTE_PROVIDER`) → `LocalProvider`
  (offline), never a startup error. `/ask` still works, deterministically.
- `LIGHT_REMOTE_PROVIDER` names a provider whose key is absent → warn and select **offline**
  (`LocalProvider`); never route to a *different* keyed provider silently (matches otto's
  `present_or_warn`).
- `LIGHT_REMOTE_PROVIDER` is an unknown value → warn and fall through to key precedence.
- `LIGHT_OPENAI_BASE_URL`/`LIGHT_DEEPSEEK_BASE_URL` rejected by `validate_base_url` → warn and fall
  through to offline; the API key is never sent. `preflight`-style validation runs at selection
  time in the TUI, so a rejected base URL is visible on the client, not silently swallowed.
- A completion fails (network, 4xx/5xx, redirect) → the error text is appended to the activity log
  as `[ask] …`; the user stays in the connected screen.
- `/ask` with an empty prompt → a one-line hint, no network call.

## Risks & Open Questions

- **Low — providers run client-side for now.** API keys live in the TUI's process environment, not
  the server. This matches the brief ("wire them into the TUI") and otto's model, but the
  long-term home is the server-side engine (the token-metering loop and shared credentials).
  `build_provider_from_env()` is written so the server reuses it unchanged.
- **Low — `candle` deferred.** A later milestone that wants in-process local inference will port
  `CandleProvider` (plus its feature gates and deps) as a separate change.
- **Low — `reqwest` version drift.** light-factory pins reqwest 0.13 (feature `rustls`); otto used
  0.12 (`rustls-tls`). The port compiles against the workspace reqwest; if any base-URL/proxy API
  differs between 0.12 and 0.13 it is caught by the ported unit tests.
