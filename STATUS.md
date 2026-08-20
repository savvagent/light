# light-factory — Project State (as of restart)

## What this is

An agentic coding platform, built in Rust, with a human-in-the-loop model. Named
**light-factory**. A response to two prior attempts:

- **otto** (https://github.com/robhicks/otto) — Rust engine, too aggressive (applied edits autonomously).
- **dark-agent** (/home/robhicks/dev/dark-agent) — ratatui TUI that hosted Claude Code; the "dark factory" idea.

## Locked decisions

- **Human in the loop**: plan-first approval + risk-tiered gates. The human owns
  planning/architecture/judgment; the agent owns mechanical execution within approved
  guardrails.
- **Architecture**: client/server protocol from day one (Command/Event over WebSocket).
- **Backend**: Rust + **Axum** (chosen over Actix Web; "TopCoat" was not a known framework).
- **Frontend (web)**: **Svelte 5** SPA — used **only for sign up and sign in** for now.
- **Agentic coding UI**: **ratatui TUI** — keep it, but restrict agentic coding to the TUI.
  (Both web + TUI coexist; web = auth only, TUI = coding.)
- **Auth**: mandatory registration + sign-in. **TOTP** for individual developers.
  Corporate users must be supported (SSO seam planned, not built).
- **Database**: **PostgreSQL** (via sqlx 0.9). Local dev cluster at 127.0.0.1:5432.
- **Deploy**: frontend (Svelte SPA) -> **Cloudflare Pages**; backend (Axum server + WebSocket)
  -> **fly.io** (flyctl authed as rob@hixfamily.org, v0.4.83), using the **existing fly Postgres
  cluster**. Considered a "serverless triangle" (Cloudflare Workers/GCR/Neon) but rejected: the
  stateful WebSocket agent is a poor fit for scale-to-zero, and Rust-on-Workers can't run
  sqlx/tokio. Kept Rust + Axum.
- **Revenue model**: **token-based** (users have a token balance; consumed as the agent works).
- **Speed/memory**: single tokio runtime, streaming, no JS runtime on the server.

## Crate layout (dependency flow inward)

```
crates/protocol      wire types (auth DTOs, Command/Event) — serde only, no I/O   [DONE]
crates/auth          domain: password policy/argon2, TOTP, AES-GCM secrets-at-rest,
                     token mint/hash, Store trait, AuthService                      [DONE + 11 tests green]
crates/persistence   sqlx Postgres PgStore + migrations                            [DONE + 1 integration test green]
crates/server        axum HTTP + WebSocket (register/login/totp/me/ws)             [DONE + smoke-tested]
crates/tui           ratatui client (agentic coding)                               [DONE — sign-in + ws]
web/                 Svelte 5 (signup/signin + TOTP QR)                            [DONE + e2e-tested]
```

## What is implemented and verified

### protocol crate
- `crates/protocol/src/auth.rs`: RegisterRequest, LoginRequest, UserView, AuthResponse,
  TotpSetupResponse, TotpConfirmRequest, LoginChallengeResponse, LoginTotpRequest,
  ErrorBody/ErrorDetail. UserView carries `token_balance` (revenue model).
- `crates/protocol/src/wire.rs`: ClientMessage/ServerMessage (Ping/Pong/Ready/Error) —
  the seam for the future Command/Event protocol.

### auth crate (light-factory-auth) — compiles, 11/11 tests pass
- `error.rs`: AuthError (stable `code()` per variant), StoreError (Duplicate/NotFound/InsufficientTokens/Other).
- `password.rs`: argon2id hash/verify; policy = min 12 chars, upper+lower+digit, max 128.
- `secret.rs`: SecretCipher (AES-256-GCM), encrypt/decrypt base64(nonce||ct).
- `token.rs`: mint_token (32B base64url), hash_token (SHA-256 hex).
- `totp.rs`: RFC 6238 SHA-1/6-digit/30s via totp-rs 5.7, issuer "light-factory".
- `store.rs`: async Store trait + User/NewUser/Session/LoginChallenge types.
- `service.rs`: AuthService (register, login, login_totp, begin_totp_setup, confirm_totp,
  authenticate, logout, issue_session). Config has session_ttl, login_challenge_ttl,
  starter_tokens, require_totp. Email normalized lowercase/trim.
- Tests in `crates/auth/tests/service.rs` (in-memory MemStore): register/duplicate/password
  policy/login/totp round-trip/require_totp/logout/token hash/secret cipher.

### persistence crate (light-factory-persistence) — PARTIAL
- `src/lib.rs`: PgStore implementing Store (sqlx query_as with UserRow/SessionRow/ChallengeRow
  FromRow rows). create_user, get_user_by_email/id, set_totp_secret, enable_totp,
  create/get/delete_session, create/consume_login_challenge (DELETE...RETURNING single-use),
  consume_tokens (UPDATE...WHERE token_balance>=amount RETURNING). map_err translates
  unique_violation -> Duplicate, RowNotFound -> NotFound.
- `migrations/0001_auth.sql`: users (email unique, totp_secret_enc, totp_enabled,
  token_balance CHECK>=0), sessions (token_hash PK, user_id FK CASCADE), login_challenges.
- `tests/pg_store.rs`: full round-trip integration test (reads DATABASE_URL, skips if no PG).
- Fixed: added `macros` to sqlx features (root Cargo.toml); `run_migrations` returns
  `sqlx::migrate::MigrateError`; added `tokio` dev-dependency. Builds clean, integration
  test green against local PG.

### server crate (light-factory-server) — compiles, smoke-tested end-to-end
- `src/config.rs`: env-driven config. `LIGHT_SECRET_KEY` (base64 32B, fail-closed),
  `REQUIRE_TOTP` (default true), `STARTER_TOKENS` (default 1000), `ADDR` (default
  127.0.0.1:8080), `DATABASE_URL` (default local dev cluster).
- `src/error.rs`: `ApiError` -> `ErrorBody` envelope with status mapping (400/401/403/
  409/402/500). JSON-extraction failures -> `invalid_json` (400).
- `src/auth_extract.rs`: `AuthenticatedUser` (`FromRequestParts`, Bearer) + `JsonBody`
  (typed JSON with the shared envelope).
- `src/routes.rs`: all routes from the plan. Login returns a union
  (`AuthResponse` w/ `token` | `LoginChallengeResponse` w/ `login_token`).
- `src/ws.rs`: `GET /ws` (Bearer header or `?token=`), sends `Ready`, echoes
  `Ping` -> `Pong`, `Error` on bad messages.
- Smoke-tested against local PG (register/duplicate/me/totp setup+confirm/login
  challenge/login-totp single-use/logout/401s/ws ready+pong+401). See below.

### web (Svelte 5 SPA) — builds, e2e-tested in a real browser
- `web/` is a Vite + Svelte 5 SPA (runes), no SSR, no Tailwind (plain CSS theme).
- `src/lib/api.js`: fetch wrapper; parses the `ErrorBody` envelope into `ApiError`.
- `src/lib/auth.js`: session store persisted to localStorage.
- `src/views/`: SignIn (password step + TOTP challenge), SignUp, TotpSetup
  (QR via `qrcode` + manual secret), Dashboard (token balance + logout).
- `src/App.svelte`: view state machine (signin/signup/totp-setup/dashboard).
- `VITE_API_URL` points at the Rust server (default `http://localhost:8080`).
- Verified end-to-end with Playwright (chromium): signup -> TOTP QR setup ->
  confirm -> dashboard (balance) -> sign out -> sign in with TOTP challenge.

### tui crate (light-factory-tui) — builds, installs, sign-in + WebSocket verified
- Binary `light-factory` (package `light-factory-tui`). Install with
  `cargo install --path crates/tui` (puts `light-factory` on PATH) or run with
  `cargo run -p light-factory-tui`.
- `src/config.rs`: `--url` / `LIGHT_API_URL` (default `http://localhost:8080`),
  maps `http(s)://` -> `ws(s)://` for the `/ws` endpoint.
- `src/session.rs`: persists the bearer token to `$XDG_CONFIG_HOME/light-factory/
  session.json` (`--logout` clears it).
- `src/api.rs`: reqwest client for register/register-confirm/login/me/logout with
  the shared `ErrorBody` envelope.
- `src/ws.rs`: tokio-tungstenite connect to `GET /ws?token=...`; pumps inbound
  `ServerMessage` into the UI loop and forwards outbound `ClientMessage`.
- `src/app.rs`: ratatui screens — SignIn (email + TOTP code), Register (email +
  name -> secret/otpauth URL -> confirm code), Connected (user header + activity
  log; `p` ping / `o` sign out / `q` quit; 30s keepalive ping). Resumes a saved
  session by validating it against `/auth/me`.

## Environment facts

- Rust 1.95.0 stable (rust-toolchain.toml pins 1.95.0). Edition 2024.
- Local PostgreSQL 16.14 installed; NO sudo. Dev cluster:
  - Data dir: `/home/robhicks/dev/light/.pgdata` (initdb -U light --auth=trust, UTF8, C.UTF-8)
  - Running: `pg_ctl -D /home/robhicks/dev/light/.pgdata -o "-p 5432 -k /tmp/opencode -c listen_addresses=127.0.0.1" -l /tmp/opencode/pg.log start`
  - DB `light`, user `light`, host 127.0.0.1:5432, DATABASE_URL `postgres://light@127.0.0.1:5432/light`
  - It may need restarting on a fresh machine/session.
- flyctl v0.4.83 at /home/robhicks/.fly/bin/flyctl, authed as rob@hixfamily.org.
- fly Managed Postgres (MPG) cluster `savvagent-pg` (ID `kyzl60xmdjxopj9g`, org `savvagent`,
  region `iad`, plan `basic`). Shared with `nels-api` (which uses `fly-db`/`fly-user`).
  light-factory uses its own database `light_factory` + user `light-factory` (MPG rejects `_`
  in usernames). Attach: `flyctl mpg attach kyzl60xmdjxopj9g --app light-factory \
  --database light_factory --username light-factory` (sets the `DATABASE_URL` secret).
- No docker; podman available.
- Cached crates (offline-friendly): sqlx 0.9.0, totp-rs 5.7.1, axum 0.8.9, ratatui 0.30.2,
  crossterm 0.29.0, tower-http 0.6.11, reqwest 0.13.4, argon2 0.5.3, aes-gcm 0.10.3, etc.
- Network to crates.io/index.crates.io/static.crates.io works (200).

## Next steps (in order)

1. ~~Fix sqlx `migrate` feature issue in root Cargo.toml (add `macros` feature).~~ DONE
2. ~~Build persistence + run `cargo test -p light-factory-persistence` against local PG.~~ DONE
3. server crate (light-factory-server): axum routes:
   - POST /auth/register -> AuthResponse
   - POST /auth/totp/setup (Bearer) -> TotpSetupResponse
   - POST /auth/totp/confirm (Bearer, {code}) -> ()
   - POST /auth/login -> LoginChallengeResponse | AuthResponse
   - POST /auth/login/totp -> AuthResponse
   - GET /auth/me (Bearer) -> UserView
   - POST /auth/logout (Bearer)
   - GET /ws (Bearer) -> WebSocket (ServerMessage::Ready/Pong)
   - GET /health
   - Wire SecretCipher from LIGHT_SECRET_KEY (32B base64), Config (require_totp default true),
     starter_tokens default. CORS for the Svelte dev server. Error envelope = ErrorBody.
   ~~DONE~~ (compiles, clippy clean, smoke-tested against local PG)
4. Scaffold Svelte 5 web app under web/ (sign up -> TOTP QR setup -> sign in with TOTP).
   ~~DONE~~ (builds; e2e-tested in a real browser against the local server)
5. Deploy (frontend Cloudflare Pages + backend fly.io, existing fly Postgres):
   - `Dockerfile` (multi-stage: rust:1.95-slim build -> debian:bookworm-slim runtime).
   - `fly.toml` (app `light-factory`, port 8080, /health check, ADDR/RUST_LOG/REQUIRE_TOTP/
     STARTER_TOKENS/CORS_ORIGINS). Secrets set via `fly secrets set` (DATABASE_URL ->
     existing PG cluster, LIGHT_SECRET_KEY).
   - Web: `wrangler` devDep + `npm run deploy` (`wrangler pages deploy dist`), `.env.production`
     baking `VITE_API_URL=https://light-factory.fly.dev`. CORS restricted to the Pages origin.
   - ~~BACKEND DONE~~ — deployed, `/health` 200, DNS `light-factory.fly.dev`.
   - ~~FRONTEND DONE~~ — deployed, `https://light-factory.pages.dev` (production), bundle
     bakes `VITE_API_URL=https://light-factory.fly.dev`, CORS preflight from the Pages origin
     verified (allow-origin matches).
6. Then the engine core (agentic coding, plan-first + risk-tiered gates) driving
   the TUI, plus Slack/JIRA/GitHub integrations and the token-metering loop.

## Key conventions

- No comments unless asked (but doc comments on pub items are okay/encouraged).
- Tests inline or in tests/; offline-deterministic where possible (MemStore pattern).
- fail-closed auth, no user enumeration (login returns InvalidCredentials for unknown email),
  secrets never returned to clients, passwords/TOTP never logged.
- `cargo fmt`, `cargo clippy -D warnings`, `cargo test` before calling a slice done.
- Do not commit unless asked. No attribution/Co-Authored-By anywhere.
