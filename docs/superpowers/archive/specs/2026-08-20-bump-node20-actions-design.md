# Bump Node 20-targeting GitHub Actions to Node 24 design

> **Status:** IMPLEMENTED — clear remaining Node 20 runtime deprecation warnings in CI by bumping the
> actions themselves, not the `node-version` pin.
>
> **Implements:** savvagent/light#9

## Premise corrections

Issue #9 was filed after #5/#6 pinned `node-version: 24` and still observed a deprecation
annotation. The premise that survives contact with the repo is: **the warning is about the actions'
own runtime, not the Node toolchain the jobs install.** `actions/checkout@v4` ships an `action.yml`
with `runs.using: node20`; GitHub's runner forces it onto Node 24 and emits
"Node.js 20 is deprecated … actions/checkout@v4". `node-version: 24` on `actions/setup-node` was
necessary but not sufficient. The fix is to bump every action that still declares a `node20`
runtime to a `node24`-targeting release.

## Scope

**In:**

- Bump every action in `.github/workflows/` that still targets the Node 20 runtime to the lowest
  major version that targets Node 24 (or, where a newer patch of the same line exists, that patch).
- Keep all existing workflow behavior identical (same jobs, same steps, same inputs, same
  `node-version: 24`).

**Out:**

- No change to Rust code, `Cargo.toml`, `Cargo.lock`, `web/`, `Dockerfile`, `fly.toml`, or
  `crates/persistence/migrations/`.
- No change to workflow logic, concurrency, triggers, or secrets plumbing.
- No version bump of any crate (no public interface or wire-type change).

## §1 Action runtime audit and target versions

Audit of every `uses:` in `.github/workflows/` against each action's `action.yml` `runs.using`
(retrieved from `raw.githubusercontent.com` at the pinned ref):

| Action | Current | `using` | Target | `using` | Files |
|---|---|---|---|---|---|
| `actions/checkout` | `@v4` | node20 | `@v5` | node24 | deploy.yml (3), release.yml (3), bump.yml (1) |
| `actions/setup-node` | `@v4` | node20 | `@v5` | node24 | deploy.yml (1) |
| `cloudflare/wrangler-action` | `@v3` | node20 | `@v4` | node24 | deploy.yml (1) |
| `superfly/flyctl-actions/setup-flyctl` | `@1.5` | node20 | `@1.6` | node24 | deploy.yml (1) |
| `actions/upload-artifact` | `@v4` | node20 | `@v6` | node24 | release.yml (3) |
| `actions/download-artifact` | `@v4` | node20 | `@v7` | node24 | release.yml (1) |
| `softprops/action-gh-release` | `@v2` | node20 | `@v3` | node24 | release.yml (1) |

Unchanged (already Node-24 or runtime-free):

- `dtolnay/rust-toolchain@stable` — `runs.using: composite`, no JS runtime, no warning.
- `Swatinem/rust-cache@v2` — `runs.using: node24`.

Non-obvious version jumps, recorded because they are where a naive "bump to next major" would
still land on Node 20:

- `actions/upload-artifact@v5` still declares `node20`; Node 24 begins at `@v6`.
- `actions/download-artifact@v5` and `@v6` still declare `node20`; Node 24 begins at `@v7`.
- `superfly/flyctl-actions/setup-flyctl@1.6` is a Git tag (no matching GitHub release) and is a
  valid `uses:` ref; `master` also targets node24. It is the newest available node24 build.

## §2 Input compatibility (no behavior change)

Each bump is chosen so the inputs already in the workflows remain valid at the target version:

- `actions/setup-node@v5` keeps `node-version`, `cache`, `cache-dependency-path`.
- `cloudflare/wrangler-action@v4` keeps `apiToken`, `accountId`, `command`, `workingDirectory`.
- `superfly/flyctl-actions/setup-flyctl@1.6` keeps `version`.
- `actions/upload-artifact@v6` keeps `name`, `path`.
- `actions/download-artifact@v7` keeps `path`, `merge-multiple`.
- `softprops/action-gh-release@v3` keeps `tag_name`, `name`, `generate_release_notes`, `files`.
- `actions/checkout@v5` keeps `ref` (used in release.yml).

Artifact handoff in `release.yml` (three `upload-artifact` jobs → one `download-artifact` with
`merge-multiple`) stays compatible: all of upload-artifact v4+ and download-artifact v4+ use the
same artifact v4 backend; the node-runtime bumps do not change the artifact format.

## §3 Security properties

This change does not touch the auth spine, secrets, or any application code. The only security
surface is CI configuration, so the review focuses on: no secrets introduced or re-plumbed, no
new shell injection (no new `run:` blocks with untrusted interpolation), no widened permissions,
and no pin change that pulls an unvetted third-party action (all targets are official
`actions/*`, or `cloudflare`, `superfly`, `softprops`, `dtolnay`, `Swatinem` — the same vendors
already in use, at newer versions).

## §4 Testing and verification

There is no local harness for GitHub Actions. Verification is:

1. Each modified `.github/workflows/*.yml` parses (`python3 -c "import yaml; yaml.safe_load(...)"`).
2. `git diff` shows only `uses:` ref changes and nothing else (no `run:` edits, no secret edits).
3. The next `push` to `master` (and the PR's own `pull_request` CI job) runs with no Node runtime
   deprecation annotations — this is the acceptance criterion and can only be observed on the
   hosted runner.

## Assumptions

1. **Lowest-Node-24-major is the right target, not latest-major.** Rationale: the issue names
   `@v5` for checkout; pinning to the first node24 major minimizes behavioral delta while still
   clearing the warning. (For upload/download-artifact, the first node24 majors are v6/v7.)
2. **`setup-flyctl@1.6` is acceptable even though it has no GitHub release.** Rationale: GitHub
   Actions resolves any tag/commit; `1.6`'s `action.yml` is node24 and is the newest such build.
3. **The artifact backend is unchanged across v4→v6/v7.** Rationale: upload/download-artifact have
   shared the "artifact v4" backend since v4; documented compat. If a future release changed this
   it would be flagged by the hosted CI, not this repo.
4. **No crate version bump is required.** Rationale: no Rust public interface or wire type changes.

## Goal & Success Criteria

Goal: CI/CD completes with no Node runtime deprecation annotations.

- [x] Every `uses:` in `.github/workflows/` resolves to a version whose `action.yml` declares a
      Node 24 (or composite) runtime — no `node20` remains.
- [x] The `CI and Deploy` workflow (and `Release`, `Bump Version`) still specify the same jobs,
      steps, and inputs; `git diff` shows only `uses:` ref changes.
- [x] All three workflow files parse as valid YAML.
- [x] PR CI (the `pull_request` trigger) runs green with no deprecation annotations.

## Error Handling & Edge Cases

- A future action major drops an input we rely on → surfaced by hosted CI; would be a separate fix.
- `download-artifact` `merge-multiple` deprecation → not triggered (still supported at v7).
- `setup-flyctl@1.6` tag removed by upstream → hosted CI fails fast at job start; recoverable by
  re-pinning to a released tag.

## Risks & Open Questions

- **Low:** the hosted runner may flag an action we missed. Mitigation: the audit in §1 enumerated
  every `uses:` in the repo; the PR CI itself is the final check.
- **Low:** `@1.6` is not a "release" per GitHub's UI, which some tooling treats specially. GitHub
  Actions does not; noted here for visibility.
