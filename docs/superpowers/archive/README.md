# Archived specs and plans

Design specs and implementation plans for work that has **shipped**. They are moved here on
close-out so `docs/superpowers/specs/` and `docs/superpowers/plans/` list only active work.

Archiving is not deletion. These remain the durable record of *why* a decision was made, which
the code itself does not carry. Some still bind current work — the two localization specs define
the locale resolution order, the fallback chain, and the `EN`/`ES` key-parity rule that any new
user-facing string must follow.

Every document here is `IMPLEMENTED` with its plan steps complete. Anything still in progress
belongs in the active directories.

| Archived | What shipped |
|---|---|
| `ci-node-24` | Pinned Node 24 in the CI/CD workflow (plan only; no spec — the change was a one-liner) |
| `bump-node20-actions` | Bumped node20 actions to node24 to clear runtime deprecation warnings |
| `pin-actions-to-shas` | Pinned every `uses:` ref to a full 40-char commit SHA; the convention now lives in `ARCHITECTURE.md` |
| `localize-tui` | `en` + `es` catalogs, locale resolution, and persistence for the ratatui client |
| `localize-web-client` | The same contract for the Svelte SPA |
