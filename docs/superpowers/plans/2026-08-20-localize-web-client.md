# Localize the web client Implementation Plan

For agentic workers: REQUIRED SUB-SKILL — `superpowers:subagent-driven-development` /
`executing-plans`. Each task below is a `- [ ]` checklist; work failing-test-first, check boxes as
you complete steps, commit at the final step of each task.

## Goal

Externalize every user-facing string in the `web/` Svelte SPA to an `en`/`es` translation catalog,
add locale resolution (saved preference → `navigator.language` → `en`) with `localStorage`
persistence and English fallback, and translate server errors by stable code.

## Architecture

A hand-rolled two-file i18n layer:

- `web/src/lib/i18n.js` — pure ESM data + functions (catalog, resolution, `translate`,
  `translateErrorCode`). No Svelte imports so it is directly unit-testable with node.
- `web/src/lib/i18n.svelte.js` — Svelte 5 runes glue: a `$state` locale proxy, `setLocale`, and
  the `t`/`errorMessage` helpers views call. `t` reads the `$state` signal so template calls are
  reactive.

Views import `t`/`errorMessage` and replace literals. `App.svelte` adds the language `<select>`.

## Tech Stack

Svelte 5 (runes), Vite 6, plain JS modules. No new dependencies.

## Spec

`docs/superpowers/specs/2026-08-20-localize-web-client-design.md` — read it first. This plan
implements it exactly.

## Global Constraints

- No AI/Claude self-attribution anywhere (comments, docs, strings).
- No new runtime dependencies (catalog is hand-rolled).
- `es` strings are full translations; missing keys fall back to `en`.
- Do not change the auth spine, API client contract, or wire types.
- Web build must pass: `cd web && npm run build`.

## File Structure

| File | Responsibility |
|---|---|
| Create. `web/src/lib/i18n.js` | Catalog (`en`/`es`), `normalizeLocale`, `resolveLocale`, `translate`, `translateErrorCode` |
| Create. `web/src/lib/i18n.svelte.js` | Reactive `locale` `$state`, `setLocale`, `t`, `errorMessage` |
| Create. `web/src/lib/i18n.test.mjs` | Node test asserting catalog completeness + fallback + resolution |
| Modify. `web/src/App.svelte` | Language selector + externalized subtitles/footers |
| Modify. `web/src/views/SignIn.svelte` | Externalized labels/buttons/errors |
| Modify. `web/src/views/SignUp.svelte` | Externalized labels/buttons/errors |
| Modify. `web/src/views/TotpSetup.svelte` | Externalized labels/buttons/alt/errors |
| Modify. `web/src/views/Dashboard.svelte` | Externalized labels/buttons |
| Modify. `web/src/views/DeviceApprove.svelte` | Externalized labels/buttons/notices/errors |

## Task Order & Rationale

1. **i18n core** first — everything else depends on the catalog and its API, and the test locks
   the catalog-completeness contract before views start consuming keys.
2. **Reactive glue** — thin, depends only on the core.
3. **App shell** — wires the selector and the two subtitle/footer sites shared across flows.
4. **Views** — mechanical literal → `t()` substitution, one commit each or grouped.
5. **Build + grep verification** — prove no English literal remains in the UI path.

## Task 1: i18n core module + tests

**Files:** `web/src/lib/i18n.js`, `web/src/lib/i18n.test.mjs`
**Interfaces:** produces `DEFAULT_LOCALE`, `SUPPORTED_LOCALES`, `STORAGE_KEY`, `catalogs`,
`normalizeLocale`, `resolveLocale`, `translate`, `translateErrorCode` (all exported from `i18n.js`).

- [ ] Write `web/src/lib/i18n.test.mjs` first (failing): import from `./i18n.js`; assert (a)
  `catalogs.es` has every key in `catalogs.en` and no extra keys; (b) `normalizeLocale('es-MX') === 'es'`,
  `normalizeLocale('fr') === null`; (c) `resolveLocale({saved:'es', navigatorLanguage:'en'}) === 'es'`,
  `resolveLocale({}) === 'en'`; (d) `translate('es','signin.title')` returns a non-English value,
  `translate('es','error.invalid_credentials')` returns a non-English value; (e) `translate('es','nope')`
  falls back to `translate('en','nope')`; (f) interpolation `translate('en','device.approve.with_code',{code:'AB12'})`
  contains `AB12`; (g) `translateErrorCode('es','invalid_credentials')` is non-null and
  `translateErrorCode('es','unknown')` is `null`.
- [ ] Run `node web/src/lib/i18n.test.mjs` — expect failure (module missing).
- [ ] Implement `web/src/lib/i18n.js` with the full `en`/`es` catalogs (keys listed in §1 of the
  spec; ensure every view string has a key).
- [ ] Run `node web/src/lib/i18n.test.mjs` — expect green.
- [ ] Format and commit: `git add web/src/lib/i18n.js web/src/lib/i18n.test.mjs && git commit -m "web: add i18n catalog and resolution"`.

## Task 2: Reactive glue

**Files:** `web/src/lib/i18n.svelte.js`
**Interfaces:** consumes `i18n.js`; produces `locale`, `setLocale`, `t`, `errorMessage`.

- [ ] Implement `web/src/lib/i18n.svelte.js` per §3/§4 of the spec (rune `$state`, best-effort
  `localStorage` read/write, `t`, `errorMessage`).
- [ ] Run `node web/src/lib/i18n.test.mjs` (unchanged) to confirm no regression.
- [ ] Format and commit: `git commit -m "web: add reactive i18n store"`.

## Task 3: App shell + language selector

**Files:** `web/src/App.svelte`
**Interfaces:** consumes `t`, `locale`, `setLocale` from `i18n.svelte.js`.

- [ ] Replace subtitles and footer literals with `t(...)` calls.
- [ ] Add a `<select>` bound to `locale.value` with `English`/`Español` options, wired to
  `setLocale` on change.
- [ ] Run `cd web && npm run build` — expect success.
- [ ] Format and commit: `git commit -m "web: localize app shell and add language selector"`.

## Task 4: Localize views

**Files:** `web/src/views/SignIn.svelte`, `web/src/views/SignUp.svelte`,
`web/src/views/TotpSetup.svelte`, `web/src/views/Dashboard.svelte`,
`web/src/views/DeviceApprove.svelte`
**Interfaces:** consumes `t`, `errorMessage`.

- [ ] Replace every literal label/button/subtitle/alt/notice with `t(...)`.
- [ ] Replace `error = e.message` with `error = errorMessage(e.code, e.message)` in every `catch`.
- [ ] Run `cd web && npm run build` — expect success.
- [ ] Format and commit: `git commit -m "web: externalize view strings to the i18n catalog"`.

## Task 5: Verification

**Files:** none (verification only)
**Reminders:** this change touches `web/` (deploy shape) — the out-of-band check is `npm run build`
plus a grep that no English literal remains in the UI path.

- [ ] `cd web && npm run build` — green.
- [ ] Grep `web/src` for remaining hardcoded English literals (labels/buttons/subtitles) — none in
  the UI path.
- [ ] `node web/src/lib/i18n.test.mjs` — green.
- [ ] Commit any remaining doc changes if needed.
