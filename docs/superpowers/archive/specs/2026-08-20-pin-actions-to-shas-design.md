# Pin GitHub Actions to immutable commit SHAs design

> **Status:** IMPLEMENTED — pin every `uses:` ref in `.github/workflows/` to a full 40-char commit SHA
> to remove the mutable-tag supply-chain risk, and document the pinning convention.
>
> **Implements:** savvagent/light#12

## Premise corrections

Issue #12 (raised during the independent security review of #11) reports that every `uses:` ref is a
mutable tag (e.g. `actions/checkout@v5`, `cloudflare/wrangler-action@v4`,
`softprops/action-gh-release@v3`, `superfly/flyctl-actions/setup-flyctl@1.6`). That premise survives
contact with the repo: all three workflows (`deploy.yml`, `release.yml`, `bump.yml`) pin by tag or
branch, none by SHA. The acceptance criteria scope the fix to "secret/privilege-handling actions",
but the mutable-tag risk is uniform across every action — a force-pushed tag or a compromised
upstream repo substitutes code regardless of whether that specific step holds a secret. See
Assumptions §1 for why this spec pins **all** actions, not just the named subset.

## Scope

**In:**

- Pin every `uses:` in `.github/workflows/` to a full 40-char commit SHA (not a tag, branch, or
  abbreviated SHA).
- Document the SHA-pinning convention in `STATUS.md` (Key conventions), including how to resolve a
  tag to its SHA when bumping an action.
- Keep all workflow behavior identical (same jobs, steps, inputs, secrets, permissions, triggers).

**Out:**

- No change to Rust code, `Cargo.toml`, `Cargo.lock`, `web/`, `Dockerfile`, `fly.toml`, or
  `crates/persistence/migrations/`.
- No change to workflow logic, concurrency, triggers, or secrets plumbing.
- No Dependabot / Renovate configuration (dependency-bump automation is a separate concern, not in
  the AC).
- No version bump of any crate (no public interface or wire-type change).

## §1 Action → SHA pin matrix

Every `uses:` ref resolved to the full commit SHA of the ref currently in use. Resolution was via
`git ls-remote` with `refs/tags/<tag>^{}` (dereferencing annotated tags to the commit they point to);
lightweight tags and branches resolve directly. The SHA recorded below is the *commit* SHA, which is
what a `uses:` line must reference.

| Action | Current ref | Pinned SHA | Files |
|---|---|---|---|
| `actions/checkout` | `@v5` | `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` | deploy.yml (3), release.yml (3), bump.yml (1) |
| `actions/setup-node` | `@v5` | `a0853c24544627f65ddf259abe73b1d18a591444` | deploy.yml (1) |
| `cloudflare/wrangler-action` | `@v4` | `ebbaa1584979971c8614a24965b4405ff95890e0` | deploy.yml (1) |
| `superfly/flyctl-actions/setup-flyctl` | `@1.6` | `ed8efb33836e8b2096c7fd3ba1c8afe303ebbff1` | deploy.yml (1) |
| `dtolnay/rust-toolchain` | `@stable` | `4360b52568e2003a75bf9bc1d59f33a8e3fc893c` | deploy.yml (1), release.yml (3) |
| `Swatinem/rust-cache` | `@v2` | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` | deploy.yml (1), release.yml (3) |
| `actions/upload-artifact` | `@v6` | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` | release.yml (3) |
| `actions/download-artifact` | `@v7` | `37930b1c2abaa49bbe596cd826c3c89aef350131` | release.yml (1) |
| `softprops/action-gh-release` | `@v3` | `3d0d9888cb7fd7b750713d6e236d1fcb99157228` | release.yml (1) |

Notes:

- `actions/*`, `actions/checkout@v5`, `actions/setup-node@v5`, `actions/upload-artifact@v6`,
  `actions/download-artifact@v7`, `cloudflare/wrangler-action@v4`, and
  `superfly/flyctl-actions/setup-flyctl@1.6` are lightweight tags: the tag object IS the commit.
- `softprops/action-gh-release@v3` and `Swatinem/rust-cache@v2` are annotated tags: the pinned SHA is
  the dereferenced commit (`^{}`), not the tag object.
- `dtolnay/rust-toolchain@stable` is a *branch*, not a tag. Pinning it to the branch tip commit
  (`4360b525…`) freezes the "stable" tracking; see Risks & Open Questions §2.
- The `superfly/flyctl-actions/setup-flyctl@1.6` pin keeps the full sub-action path and replaces only
  the `@1.6` suffix with the SHA — the `setup-flyctl` sub-action is the node24 one; the root
  `flyctl-actions` action is Docker-based and is NOT used.

## §2 Pin format and the full-SHA requirement

A pinned `uses:` reads `owner/repo@<full-40-char-sha>` (e.g.
`cloudflare/wrangler-action@ebbaa1584979971c8614a24965b4405ff95890e0`). GitHub Actions resolves
immutably to that commit. Abbreviated SHAs (8–12 chars) are ambiguous and rejected by the runner when
pinning to a commit in this syntax; the convention below therefore mandates the full 40-char SHA.

## §3 Security properties

This change is itself a supply-chain hardening: after it lands, a force-pushed tag or compromised
upstream repo no longer substitutes code into a workflow run, because the runner fetches the exact
commit. Specifically:

- Actions that hold live secrets (`wrangler-action` with `CLOUDFLARE_API_TOKEN`/`CLOUDFLARE_ACCOUNT_ID`,
  `setup-flyctl` with `FLY_API_TOKEN`) now run only the exact audited commit.
- Actions under `contents: write` (`action-gh-release`, and `checkout` in `bump.yml` where a PAT is
  used to push) now run only the exact audited commit.
- No secrets, permissions, or shell blocks change; the diff is `uses:` lines and a `STATUS.md`
  convention note only.

## §4 Testing and verification

There is no local GitHub Actions harness. Verification is:

1. Each modified `.github/workflows/*.yml` parses (`python3 -c "import yaml; yaml.safe_load(...)"`).
2. `git diff` shows only `uses:` ref changes and the `STATUS.md` convention note — no `run:`, `env:`,
   `permissions:`, or secret edits.
3. Every `uses:` resolves to a 40-char commit SHA that matches the §1 matrix (re-verified with
   `git ls-remote`).
4. The next `pull_request` CI run (the PR's own `ci` job in `deploy.yml`) stays green — this is the
   "CI remains green" acceptance criterion and is observed on the hosted runner.

## Assumptions

1. **Pin ALL actions, not just the secret/privilege-handling subset named in the AC.**
   Rationale: the mutable-tag substitution risk is identical for every action, and drawing a line
   between "holds a secret" and "does not" is arbitrary and unmaintainable (e.g. `checkout` in
   `bump.yml` writes to `master` under a PAT). Pinning all is a strict superset of the AC and is the
   OpenSSF `Pinned-Dependencies` best practice.
2. **Pin to the commit SHA of the ref currently in use, not to a newer release.**
   Rationale: the AC is about immutability, not upgrades. Resolving `@v5` → its current commit keeps
   behavior byte-identical; upgrading to a newer tag is a separate, deliberate decision.
3. **`dtolnay/rust-toolchain@stable` is pinned to its current branch-tip commit.**
   Rationale: `stable` is a moving branch. Freezing it to today's tip is the immutability tradeoff;
   a future Rust toolchain bump becomes an explicit, reviewed SHA update (Risks §2).
4. **No crate version bump is required.** Rationale: no Rust public interface or wire-type change.
5. **`STATUS.md` is the right home for the convention.** Rationale: it is this repo's documented
   convention ledger ("Key conventions"); a per-workflow comment would duplicate it across three
   files and drift.

## Goal & Success Criteria

Goal: no workflow runs code reachable only through a mutable ref; every action resolves to an exact,
pinned commit.

- [x] Every `uses:` in `.github/workflows/` is a full 40-char commit SHA matching the §1 matrix.
- [x] `STATUS.md` documents the SHA-pinning convention (including the tag→SHA resolution command).
- [x] `git diff` shows only `uses:` ref changes and the convention note — no behavior change.
- [x] All three workflow files parse as valid YAML.
- [x] PR CI (the `pull_request` trigger) runs green.

## Error Handling & Edge Cases

- A pinned commit is later force-removed from an upstream repo → the runner fails fast at job start;
  recoverable by re-pinning to the new commit. This is the intended fail-closed behavior.
- An action's tag is re-pointed by its maintainer (routine release) → our pin is unaffected until we
  deliberately bump, which is the point of the change.
- `dtolnay/rust-toolchain@stable` frozen tip goes stale (a future Rust release) → a later, explicit
  SHA bump restores tracking; see Risks §2.

## Risks & Open Questions

- **Low — SHA pins rot.** Pinned SHAs never track upstream automatically; a security fix upstream
  requires a manual bump. Mitigation: the convention (Assumptions §5) records the exact resolution
  command so bumps are cheap and reviewable. Dependabot/Renovate automation is a possible follow-up,
  explicitly out of scope here.
- **Low — `rust-toolchain@stable` semantics change.** Freezing a "stable" branch to a commit means CI
  stops tracking the newest Rust stable automatically. This is accepted (Assumptions §3); if it
  becomes painful, the follow-up is a scheduled SHA-bump job, not a revert to `@stable`.
- **Low — human error on the next bump.** A future editor may re-introduce a mutable tag. Mitigation:
  the `STATUS.md` convention calls out the full-SHA requirement explicitly.
