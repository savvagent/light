# Localize the web client design

> **Status:** DRAFT — externalize every user-facing string in `web/` to an `en` + `es` catalog with locale resolution, persistence, fallback, and deliberate server-error handling.

> **Implements:** https://github.com/savvagent/light/issues/1

## Premise corrections

- The issue states "server-side error messages are handled deliberately (translated via stable
  error `code`, or surfaced verbatim)". The server already ships a stable `code` on every error
  envelope (`crates/protocol/src/auth.rs:101` `ErrorDetail.code`, populated by
  `crates/auth/src/error.rs:31` `AuthError::code()` and `crates/server/src/error.rs:18`
  `ApiError`). No server change is required; the web client only needs to consume `code`.

## Scope

**In:**
- A translation catalog for `en` and `es` covering every user-facing string in `web/src`.
- Locale resolution: saved preference → `navigator.language` → `en`.
- Persistence of the chosen locale in `localStorage`.
- A language selector control in the UI.
- English fallback for missing translations.
- Translation of server error messages via stable error `code`, with verbatim fallback.

**Out:**
- Server-side localization (error messages stay English on the wire; clients translate by code).
- RTL / pluralization / ICU message format (out of scope; only simple `{param}` interpolation).
- Locale-aware number/date formatting.
- Adding a third-party i18n framework (the catalog is hand-rolled, matching the repo's
  minimal-dependency style).
- Localizing `index.html` `<title>`/`<html lang>` beyond a static value.

## §1 Translation catalog

A single module `web/src/lib/i18n.js` (plain ESM, no Svelte imports — unit-testable in isolation)
holds:

- `DEFAULT_LOCALE = 'en'`
- `SUPPORTED_LOCALES = ['en', 'es']`
- `STORAGE_KEY = 'light_factory_locale'`
- `catalogs = { en: {…}, es: {…} }` — a flat key namespace using dot-notation groups
  (`common.email`, `signin.title`, `error.invalid_credentials`, …). Interpolation uses
  `{param}` placeholders.

`catalogs.en` is the authoritative source of truth for key names; `catalogs.es` provides full
Spanish translations. `es` strings are hand-authored (no machine translation markers per repo
attribution rules).

## §2 Lookup and fallback

- `normalizeLocale(raw)`: lowercase, take the primary subtag before `-`/`_`, return the locale if
  it is in `SUPPORTED_LOCALES`, else `null`. This means `es-MX` → `es`, `en-US` → `en`,
  `fr` → `null`.
- `resolveLocale({ saved, navigatorLanguage })`: first `normalizeLocale(saved)` (the
  `localStorage` preference), then `normalizeLocale(navigatorLanguage)`, then `DEFAULT_LOCALE`.
- `translate(locale, key, params)`: look up `key` in `catalogs[locale]`, else `catalogs.en`,
  else return the key verbatim (a loud developer sentinel — only reachable when a catalog is
  incomplete). Then substitute `{param}` placeholders from `params`. Never throws.

Fallback order therefore satisfies "missing translations fall back to English instead of rendering
keys or throwing": a missing `es` string renders its `en` counterpart; the `key` sentinel is
unreachable in production because `catalogs.en` is complete by construction (tests assert it).

## §3 Reactive store

`web/src/lib/i18n.svelte.js` (rune-based glue) exposes:

- `locale` — a `$state({ value })` proxy initialized from `resolveLocale(...)`.
- `setLocale(raw)` — normalize, assign `locale.value`, and persist to `localStorage` under
  `STORAGE_KEY` (best-effort `try/catch`, since storage can throw in private modes).
- `t(key, params)` — `translate(locale.value, key, params)`. Reading `locale.value` inside a
  component's render makes every `t(...)` call reactive to locale changes under Svelte 5 runes.
- `errorMessage(code, fallback)` — `translateErrorCode(locale.value, code) ?? fallback`.

## §4 Server error handling

`translateErrorCode(locale, code)` maps a stable `code` to `error.<code>` in the catalog. Codes
with translations: `invalid_credentials`, `email_taken`, `invalid_email`, `invalid_totp_code`,
`invalid_challenge`, `invalid_session`, `invalid_grant`, `expired_token`, `storage_error`,
`internal_error`, `invalid_json`, `network`, `decode`. All other codes (including `unknown`) are
**surfaced verbatim** from the server's `message`, which is English but safe and legitimate.

The `ApiError` produced by `web/src/lib/api.js` already carries `.code` and `.message`
(`web/src/lib/api.js:3`). Each view's `catch` changes from `error = e.message` to
`error = errorMessage(e.code, e.message)`.

**Choice (documented):** translate the known stable codes; surface anything unrecognized
verbatim. This keeps the client forward-compatible with future server error codes without
dropping information, and never invents text for a code it does not understand.

## §5 Language selector

`web/src/App.svelte` renders a small `<select>` bound to `locale.value` (via `setLocale` on
`change`) listing `English` / `Español`. It appears at the top of the card on every view, giving
the explicit user control required by the AC. The selector labels themselves are not translated
(the language names are shown in their own language).

## §6 Files

| File | Change |
|---|---|
| `web/src/lib/i18n.js` | Create — catalog, resolution, `translate`, `translateErrorCode` |
| `web/src/lib/i18n.svelte.js` | Create — reactive `locale`, `setLocale`, `t`, `errorMessage` |
| `web/src/App.svelte` | Modify — language selector + externalized subtitles/footers |
| `web/src/views/SignIn.svelte` | Modify — externalize labels/buttons/errors |
| `web/src/views/SignUp.svelte` | Modify — externalize labels/buttons/errors |
| `web/src/views/TotpSetup.svelte` | Modify — externalize labels/buttons/alt/errors |
| `web/src/views/Dashboard.svelte` | Modify — externalize labels/buttons |
| `web/src/views/DeviceApprove.svelte` | Modify — externalize labels/buttons/notices/errors |
| `web/src/lib/i18n.test.mjs` | Create — node test for pure functions |

## Assumptions

- No new runtime dependencies; the catalog is hand-rolled. Rationale: the repo's frontend has zero
  runtime deps beyond `qrcode`, and a full i18n framework is unnecessary for ~40 strings.
- Interpolation is plain `{param}` string replacement, no ICU/plurals. Rationale: the UI has no
  pluralization or complex message-format needs today.
- The language selector is a `<select>` (not a toggle) because it scales to more than two locales.
- `es` is the shipped second locale because the AC names it as the example.
- The `key`-verbatim sentinel is acceptable as a final fallback because `catalogs.en` completeness
  is enforced by a test that diffs `es` keys against `en` keys.
- Machine-translated `es` strings are fine but carry no attribution markers (repo attribution rule).

## Goal & Success Criteria

Goal: every user-facing string in the web client renders in the user's preferred language.

- `en` and `es` catalogs cover every user-facing string; no English literal remains in the UI path.
- Locale resolves from saved preference → `navigator.language` → `en`.
- Chosen locale persists across reloads in `localStorage`.
- A missing `es` key renders its English equivalent (tested); `translate` never throws.
- Known server error codes render localized text; unknown codes render the server message verbatim.

## Error Handling & Edge Cases

- `localStorage` unavailable/throws (private mode) → preference read/write is a best-effort
  `try/catch`; resolution still falls back to `navigator.language` → `en`.
- Unknown `navigator.language` (e.g. `fr`) → `en`.
- Unknown error `code` → verbatim server message.
- A `{param}` placeholder with no matching `params` entry → left as the literal `{param}` (no
  throw), documented as a developer bug.

## Risks & Open Questions

- Svelte 5 fine-grained reactivity of a `$state` read through an imported function is the expected
  behavior but should be verified with the Vite build + a manual reload.
- The `es` translations are best-effort; a native speaker should review them later.
