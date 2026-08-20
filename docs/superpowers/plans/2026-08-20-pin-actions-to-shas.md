# Pin GitHub Actions to immutable commit SHAs Implementation Plan

For agentic workers: REQUIRED SUB-SKILL — `superpowers:subagent-driven-development` and
`superpowers:executing-plans`. Read both before executing this plan. Check off each `- [ ]` step as
it is completed.

## Goal

Pin every `uses:` ref in `.github/workflows/` to a full 40-char commit SHA (no mutable tags or
branches), and record the SHA-pinning convention in `STATUS.md`, with no behavioral change to the
workflows.

## Architecture

No architectural change. CI configuration-only: the same three workflows, jobs, steps, triggers,
concurrency, inputs, secrets, and permissions remain; only the `uses:` refs move from tag/branch to
full commit SHA. The pinning convention is documented in `STATUS.md` "Key conventions".

## Tech Stack

GitHub Actions workflow YAML + a Markdown convention note. No Rust, no Svelte, no `web/`, no
`Dockerfile`/`fly.toml`/migration changes.

## Spec

`docs/superpowers/specs/2026-08-20-pin-actions-to-shas-design.md` — read it first. This plan
implements it exactly. The spec's §1 table is the authoritative pin matrix.

## Global Constraints

- All work in the worktree `.worktrees/pin-actions-to-shas`; never commit to `master` directly.
- No AI/Claude attribution in commits, PR bodies, or docs.
- Only `uses:` refs change in the workflows; no `run:`, `env:`, `with:`, `permissions:`, or secret
  edits. The only non-`uses:` change is the `STATUS.md` convention note.
- Every `uses:` must be a full 40-char commit SHA — no abbreviated SHA, no tag, no branch.
- Preserve the full sub-action path `superfly/flyctl-actions/setup-flyctl@<sha>` (root `flyctl-actions`
  is a Docker action and is NOT used; only `setup-flyctl` is).
- `dtolnay/rust-toolchain@<sha>` is pinned to the current `stable` branch tip (a branch, not a tag).

## File Structure

| File | Responsibility |
|---|---|
| Modify. `.github/workflows/deploy.yml` | Pin checkout (3), setup-node, wrangler-action, flyctl, rust-toolchain, rust-cache |
| Modify. `.github/workflows/release.yml` | Pin checkout (3), rust-toolchain (3), rust-cache (3), upload-artifact (3), download-artifact, action-gh-release |
| Modify. `.github/workflows/bump.yml` | Pin checkout (1) |
| Modify. `STATUS.md` | Add the SHA-pinning convention to "Key conventions" |

## Task Order & Rationale

Task 1 (the mechanical pin) and Task 2 (the convention note) are independent; Task 2 references the
policy Task 1 implements, so Task 1 runs first to establish the pins, then Task 2 documents them.
Each task ends with its own verification + commit so the pin and the policy land as separable,
reviewable commits.

### Task 1: Pin every action to a full commit SHA

**Files:** `.github/workflows/deploy.yml`, `.github/workflows/release.yml`, `.github/workflows/bump.yml`

**Interfaces:** consumes GitHub Actions `uses:` refs only; no code interfaces.

The pin matrix (from the spec §1):

| Action | Current ref → SHA | Files |
|---|---|---|
| `actions/checkout` | `@v5` → `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` | deploy.yml (3), release.yml (3), bump.yml (1) |
| `actions/setup-node` | `@v5` → `a0853c24544627f65ddf259abe73b1d18a591444` | deploy.yml (1) |
| `cloudflare/wrangler-action` | `@v4` → `ebbaa1584979971c8614a24965b4405ff95890e0` | deploy.yml (1) |
| `superfly/flyctl-actions/setup-flyctl` | `@1.6` → `ed8efb33836e8b2096c7fd3ba1c8afe303ebbff1` | deploy.yml (1) |
| `dtolnay/rust-toolchain` | `@stable` → `4360b52568e2003a75bf9bc1d59f33a8e3fc893c` | deploy.yml (1), release.yml (3) |
| `Swatinem/rust-cache` | `@v2` → `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` | deploy.yml (1), release.yml (3) |
| `actions/upload-artifact` | `@v6` → `b7c566a772e6b6bfb58ed0dc250532a479d7789f` | release.yml (3) |
| `actions/download-artifact` | `@v7` → `37930b1c2abaa49bbe596cd826c3c89aef350131` | release.yml (1) |
| `softprops/action-gh-release` | `@v3` → `3d0d9888cb7fd7b750713d6e236d1fcb99157228` | release.yml (1) |

- [ ] In `.github/workflows/deploy.yml`: replace `actions/checkout@v5` → `actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` (3 occurrences), `actions/setup-node@v5` → `actions/setup-node@a0853c24544627f65ddf259abe73b1d18a591444`, `cloudflare/wrangler-action@v4` → `cloudflare/wrangler-action@ebbaa1584979971c8614a24965b4405ff95890e0`, `superfly/flyctl-actions/setup-flyctl@1.6` → `superfly/flyctl-actions/setup-flyctl@ed8efb33836e8b2096c7fd3ba1c8afe303ebbff1`, `dtolnay/rust-toolchain@stable` → `dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c`, `Swatinem/rust-cache@v2` → `Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6`.
- [ ] In `.github/workflows/release.yml`: replace `actions/checkout@v5` → SHA (3 occurrences), `dtolnay/rust-toolchain@stable` → SHA (3 occurrences), `Swatinem/rust-cache@v2` → SHA (3 occurrences), `actions/upload-artifact@v6` → `actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f` (3 occurrences), `actions/download-artifact@v7` → `actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131`, `softprops/action-gh-release@v3` → `softprops/action-gh-release@3d0d9888cb7fd7b750713d6e236d1fcb99157228`.
- [ ] In `.github/workflows/bump.yml`: replace `actions/checkout@v5` → `actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` (1 occurrence).
- [ ] Verify no mutable ref remains: `grep -rn "uses:" .github/workflows/` — every line must end in a 40-char hex SHA, and no `@v\d`, `@\d+\.\d+`, `@stable`, or other tag/branch suffix remains.
- [ ] Verify the diff is `uses:`-only, per-file: `git diff -- .github/workflows/` shows no `run:`, `env:`, `with:`, `permissions:`, `on:`, or `secrets:` line changed (only `- uses:` lines whose suffix is the ref).
- [ ] Validate all three files parse:
      `python3 -c "import yaml; [yaml.safe_load(open(f)) for f in ('.github/workflows/deploy.yml','.github/workflows/release.yml','.github/workflows/bump.yml')]"`.
- [ ] Format and commit: `git commit -m "ci: pin actions to immutable commit SHAs"`.

### Task 2: Document the SHA-pinning convention in STATUS.md

**Files:** `STATUS.md`

**Interfaces:** none — documentation only.

- [ ] In `STATUS.md` "Key conventions", append a bullet stating the policy: every `uses:` in
      `.github/workflows/` is pinned to a full 40-char commit SHA (never a mutable tag/branch), and
      giving the tag→SHA resolution command
      (`git ls-remote https://github.com/<owner>/<repo>.git "refs/tags/<tag>^{}"` — dereference
      annotated tags to their commit). Include the note that `dtolnay/rust-toolchain@stable` is
      pinned to a branch-tip commit and must be bumped explicitly on toolchain changes.
- [ ] Verify `git diff -- STATUS.md` shows only the appended convention bullet, nothing else.
- [ ] Format and commit: `git commit -m "docs: document the action SHA-pinning convention"`.

## Out-of-band verification

The change touches `.github/` (CI) and `STATUS.md`. There is no local GitHub Actions harness; the
in-repo checks are YAML validity, a `uses:`-only diff, and the no-mutable-ref grep (above). The
acceptance criterion — CI remains green — is observed on the hosted runner via the PR's own
`pull_request` CI job (`deploy.yml`'s `ci`), which runs `cargo clippy` + `cargo test`. The `push`-only
deploy jobs (`cloudflare-pages`, `fly`) and the `release`/`bump` workflows are not exercised by a PR,
but their only change is the `uses:` ref suffix, so the YAML-validity + grep checks are the relevant
verification for those.
