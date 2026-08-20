# Bump Node 20-targeting GitHub Actions to Node 24 Implementation Plan

For agentic workers: REQUIRED SUB-SKILL — `superpowers:subagent-driven-development` and
`superpowers:executing-plans`. Read both before executing this plan. Check off each `- [ ]` step as
it is completed.

## Goal

Clear the remaining GitHub Actions "Node.js 20 is deprecated" annotations by bumping every
`uses:` ref in `.github/workflows/` that still declares a `node20` runtime to its lowest
Node 24-targeting release, with no behavioral change to the workflows.

## Architecture

No architectural change. CI configuration-only: the same three workflows, jobs, steps, triggers,
concurrency, and inputs remain; only the `uses:` version refs move. `node-version: 24` (from #6)
stays as-is — it was necessary but not sufficient; the warnings come from the actions' own runtime.

## Tech Stack

GitHub Actions workflow YAML. No Rust, no Svelte, no `web/`, no `Dockerfile`/`fly.toml`/migration
changes.

## Spec

`docs/superpowers/specs/2026-08-20-bump-node20-actions-design.md` — read it first. This plan
implements it exactly. The spec's §1 table is the authoritative bump matrix.

## Global Constraints

- All work in the worktree `.worktrees/bump-node20-actions`; never commit to `master` directly.
- No AI/Claude attribution in commits, PR bodies, or docs.
- No dependency-flow, wire-type, auth-spine, or deploy/distribution-shape change beyond the CI refs.
- Preserve the full sub-action path `superfly/flyctl-actions/setup-flyctl@1.6` (the root
  `flyctl-actions` action is a Docker action; only the `setup-flyctl` sub-action is node24).
- Only `uses:` refs change; no `run:` blocks, no secrets plumbing, no `permissions` edits.

## File Structure

| File | Responsibility |
|---|---|
| Modify. `.github/workflows/deploy.yml` | Bump checkout (3), setup-node, wrangler-action, flyctl |
| Modify. `.github/workflows/release.yml` | Bump checkout (3), upload-artifact (3), download-artifact, action-gh-release |
| Modify. `.github/workflows/bump.yml` | Bump checkout (1) |

## Task Order & Rationale

Single-task change; no cross-file ordering. All refs are independent, so one task edits all three
files, then a single verification + commit step closes it out.

### Task 1: Bump node20-targeting actions to node24 across workflows

**Files:** `.github/workflows/deploy.yml`, `.github/workflows/release.yml`, `.github/workflows/bump.yml`

**Interfaces:** consumes GitHub Actions `uses:` refs only; no code interfaces.

The bump matrix (from the spec §1):

| Action | From → To | Files |
|---|---|---|
| `actions/checkout` | `@v4` → `@v5` | deploy.yml (3), release.yml (3), bump.yml (1) |
| `actions/setup-node` | `@v4` → `@v5` | deploy.yml (1) |
| `cloudflare/wrangler-action` | `@v3` → `@v4` | deploy.yml (1) |
| `superfly/flyctl-actions/setup-flyctl` | `@1.5` → `@1.6` | deploy.yml (1) |
| `actions/upload-artifact` | `@v4` → `@v6` | release.yml (3) |
| `actions/download-artifact` | `@v4` → `@v7` | release.yml (1) |
| `softprops/action-gh-release` | `@v2` → `@v3` | release.yml (1) |

Do NOT touch: `dtolnay/rust-toolchain@stable` (composite), `Swatinem/rust-cache@v2` (already node24).

- [ ] In `.github/workflows/deploy.yml`: `actions/checkout@v4` → `actions/checkout@v5` (3 occurrences),
      `actions/setup-node@v4` → `actions/setup-node@v5`, `cloudflare/wrangler-action@v3` →
      `cloudflare/wrangler-action@v4`, `superfly/flyctl-actions/setup-flyctl@1.5` →
      `superfly/flyctl-actions/setup-flyctl@1.6`.
- [ ] In `.github/workflows/release.yml`: `actions/checkout@v4` → `actions/checkout@v5` (3 occurrences),
      `actions/upload-artifact@v4` → `actions/upload-artifact@v6` (3 occurrences),
      `actions/download-artifact@v4` → `actions/download-artifact@v7`,
      `softprops/action-gh-release@v2` → `softprops/action-gh-release@v3`.
- [ ] In `.github/workflows/bump.yml`: `actions/checkout@v4` → `actions/checkout@v5` (1 occurrence).
- [ ] Verify no `node20`-targeting ref remains: grep `.github/workflows/` and confirm every `uses:`
      resolves to a node24 or composite action (re-run the `runs.using` check on each pinned ref).
- [ ] Confirm `git diff` shows ONLY `uses:` line changes — no `run:`, `env:`, `permissions:`, or
      secret edits.
- [ ] Validate all three files parse:
      `python3 -c "import yaml; [yaml.safe_load(open(f)) for f in ('.github/workflows/deploy.yml','.github/workflows/release.yml','.github/workflows/bump.yml')]"`.
      (Requires PyYAML; local check only.)
- [ ] Format and commit: `git commit -m "ci: bump node20 actions to node24"`.

## Out-of-band verification

The change touches `.github/` (CI). There is no local GitHub Actions harness; the in-repo checks
are YAML validity + a `uses:`-only diff (above). The acceptance criterion — no Node runtime
deprecation annotations — is observed on the hosted runner on the next `pull_request`/`push` run
(the PR's own `ci` job exercises `deploy.yml`).
