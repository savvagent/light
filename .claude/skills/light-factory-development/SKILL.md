---
name: light-factory-development
description: Use when developing any feature or fix in the light-factory repository (the agentic coding platform) end-to-end — from a GitHub issue or plain task brief, through ship and verify, fully autonomously with no mid-run questions. Bundles the plan-by-plan discipline (committed design specs in docs/superpowers/specs/ and implementation plans in docs/superpowers/plans/), the Rust workspace conventions (inward dependency flow, the passwordless TOTP auth spine, the human-in-the-loop architecture), autonomous spec generation, plan generation, task-by-task implementation, PR review loops, verification, and close-out. For other repositories use general-development; for fs-ci use ce-development.
---

# Light-Factory Development — Autonomous End-to-End

Autonomous, plan-driven feature/fix workflow for the **light-factory** repository (the agentic
coding platform). Walks from intake (a GitHub issue OR a plain task brief) through spec → plan →
implement → PR → review → merge → verify → close, with no mid-run human questions. Returns control
only when the work is shipped + verified, or on a true blocker.

This is the **light-factory-specific sibling** of `general-development`. Same spine; the
repo-agnostic convention-discovery phase is replaced by the hardcoded conventions below (Rust
workspace, plan-by-plan docs, passwordless auth spine), the way `ce-development` hardcodes fs-ci's.
If you are working in any other repository, use `general-development`.

## Why this shape

The spine mirrors what this repository builds: light-factory is a **human-in-the-loop** agentic
platform — plan-first approval, where the human owns planning/architecture/judgment and the agent
owns mechanical execution within the approved plan's scope.
The workflow below is the same loop applied to the repo itself: a plan document first (the
approval gate), implementation against it (the mechanical execution), and the repo's own test
suite as the Verifier. `ARCHITECTURE.md` describes the *intended* destination and marks what is
built in its "Current state" table; `docs/superpowers/plans/` (latest plan) records where the build
currently stands.

## The Iron Law

**The source of truth is the task brief OR the GitHub issue — whichever started the work.** Not
Slack. Not the PR title. Not a teammate's summary. If the work began from a plain instruction, that
instruction (captured verbatim at intake) is the contract. If an issue exists, read the issue.

**Fully autonomous means no mid-run questions.** Make the most reasonable interpretation, document
the assumption, continue. Escalate only on true blockers. "Should I continue?" is never a stop
condition.

**Green tests are not the same as work-done.** `cargo test --workspace` does not validate the Fly
image (`Dockerfile`/`fly.toml`), the Svelte SPA bundle (`web/`), or the database migrations
(`crates/persistence/migrations/`). Whatever this change touches, verify it explicitly (Phase 5).

**Violating the letter of the workflow is violating the spirit.**

## Non-Negotiable Rules

These hold for every run of this skill, no exceptions, no fast-path carve-outs:

1. **All work happens in a worktree.** Never edit, commit, or stage anything in the main checkout's
   working tree. The worktree is created in Phase 0 and removed in Phase 4 step 12. There is no
   scenario in which code is written on `master` directly.
2. **`master` changes only through PRs.** Every commit to `master` lands via a reviewed, merged PR.
   Never push a commit or branch directly to `master`; never `git push origin master`; never merge
   without the PR open, reviewed, and green. The only master-branch writes this workflow performs
   are squash-merges of PRs (Phase 4 step 11).
3. **Coding agents never self-attribute.** No `Co-Authored-By` trailers, no "Generated with"
   footers, no `🤖`/`AI`/credit markers of any kind — in commit messages, PR bodies, code comments,
   docs, or READMEs. This applies to direct work and to anything delegated to a subagent. An
   otherwise-perfect commit that carries attribution is rejected and rewritten.
4. **Every PR is reviewed by a Rust expert, an architect, and an independent security agent.** No
   PR opens, merges, or is pushed through the review loop without a dedicated `rust-pro` (Rust
   expert) review, a dedicated `architect-reviewer` (architectural) review, AND a dedicated
   `security-auditor` review on record. Fast-path and trivial PRs included — there is no size-based
   carve-out. All three must pass (or their issues resolved) before merge.
5. **The security review is independent.** The `security-auditor` agent receives **only the PR
   diff** — never the spec, never the plan, never the task brief, never the PR body summary, never
   the implementer's report. Its findings must be produced from the diff alone, so it cannot be
   steered by the implementer's framing. This is deliberate: the security review is the one pass
   that evaluates what was actually built, unmediated.
6. **Semver is a requirement on every change to a public interface.** The `protocol` wire types and
   every crate's public API stay **semver-minor** (additive only): new optional serde fields, new
   enum variants, new event kinds, new auth DTOs, new route request/response shapes — the repo's
   convention that additions to the wire types are semver-minor. Additive changes need no
   `Cargo.toml` bump. A **breaking** change — renaming, removing, or reordering a field or variant;
   changing a signature or shape — is semver-major and is never an incidental refactor side-effect
   or a fast-path change. It must be named in the spec and plan, bump the affected crate(s)'
   `version` in `Cargo.toml` within the same PR (at this repo's pre-1.0 state, where every crate is
   0.x, that is the 0.x minor-position bump, `0.1.0` → `0.2.0`), and be flagged explicitly to the
   architect reviewer — the bump makes it visible to the independent security reviewer in the diff.
   A silent breaking change is a rejected PR.

## When to Use This Skill vs. Alternatives

| Situation | Use |
|---|---|
| Any feature/fix in light-factory, full lifecycle, no human in the loop | **light-factory-development** (this skill) |
| Work in another repository | `general-development` |
| Work in fs-ci / Contract Explorer | `ce-development` |
| Already mid-implementation, just need to address PR review comments | the Review-Response step here (Phase 4 step 9) |
| Spec/plan only, will hand off to a human implementer | Phases 1–2 of this skill |
| Guided mode with human approval at each checkpoint | run the phases directly, stopping at each gate |
| One-line typo fix or docs nit | Fast-path below — the full spec/plan phases are overkill |

## Fast-Path: Trivial Tasks (skip the spec + critique loops)

This repo is **plan-by-plan by house style** — but genuine triviality does not need a design spec.
Skip the spec document, the spec critique, and the plan critique ONLY when **ALL** of the following
are true:

- Single-file or 1–2 logical source files (tests and lock files don't count toward the cap; a file
  and its required mirror/duplicate count as one logical file)
- No new public interface: no new wire type or enum variant in `protocol`, no new auth DTO, no new
  route/endpoint, no new `Store` method, no new config/env key, no new CLI flag, no new crate or
  binary, no new migration
- No **breaking** change to wire types or a crate's public API — renamed/removed/reordered serde
  fields or enum variants, changed signatures. Breaking changes are semver-major by Non-Negotiable
  Rule 6 and never fast-path
- No change to the auth spine, a route handler's behavior, the device-grant flow, the
  `SecretCipher`/secret path, or session/token handling
- No change to dependency flow (no crate gains/loses an inward/outward edge)
- No behavior change on a code path covered by tests (a type-only fix is fine; a logic change that
  alters runtime behavior is not)
- No change to deploy/distribution shape (`Dockerfile`, `fly.toml`, `web/`,
  `crates/persistence/migrations/`)
- The acceptance criterion fits in one sentence

Concrete examples that qualify:
- Fix a type error with no behavior delta
- Fix a typo in a string / comment / docstring
- Rename a local variable
- Delete demonstrably dead code (verified zero production callers)
- Run `cargo fmt --all` over a crate
- Update a hardcoded constant the brief names verbatim

**Even when fast-pathing, the plan document is not skipped.** Per house style, the plan lives in the
repo — a fast-path ticket still lands a minimal single-task plan at
`docs/superpowers/plans/YYYY-MM-DD-<slug>.md` in the plan's task-checkbox form (Goal + one
`### Task` with `- [ ]` steps + TDD + commit step). The design spec and both critique loops are the
parts that are skipped. Add to the PR body: `Fast-path: no design spec per light-factory-development trivial-task criteria — <reason>.`

If you find yourself rationalizing into the fast-path on something that touches 3+ source files,
introduces a new interface, touches the auth spine, adds a dependency, or has more than a
one-sentence AC → STOP. Write the spec. The fast-path is for genuine triviality, not "I think this is small."

| Fast-path rationalization | Reality |
|---|---|
| "It's only 3 files" | Fast-path caps at 2. Three files → spec. |
| "The new endpoint is tiny" | New endpoint = new public interface = spec. Endpoints are part of the client/server protocol. |
| "I'll just read `LIGHT_SECRET_KEY` in the auth crate" | Secrets/config reads belong in the `server` crate's `config` module, fail-closed — that's a design decision, not a nit. |
| "The type fix incidentally fixes a bug" | If behavior changes, you need the spec to record what it changed and why. |
| "I'll fast-path the first sub-change and spec the rest" | If the work splits into sub-changes, write the spec. Multi-step work doesn't fast-path. |
| "No spec, but I'll still write a one-line plan" | Either the work needs a plan (then write the spec too) or it doesn't (then it doesn't need the plan either — and per house style, even fast-path keeps a minimal plan doc). |

## Repository Conventions (light-factory)

| Convention | Value |
|---|---|
| Trunk | `master`. **All** work happens in a worktree; **all** master-branch changes land via merged PRs. No direct commits, pushes, or merges to `master` outside a PR (Non-Negotiable Rules 1–2). |
| Worktree | **Required.** `git worktree add .worktrees/<branch> -b <branch> origin/master` — the worktrees live **inside the repo** at `.worktrees/<branch>`, not as sibling directories. `.worktrees/` is gitignored — **never `git add .`** in the main checkout (a nested worktree's contents, including developer-local secrets, could be staged). Branch off `origin/master`, never local `master` (see Phase 0 trunk-sync). Every code edit, commit, and push happens from inside this worktree. |
| Branch name | `<kebab-slug>` — e.g. `add-password-reset`, `device-grant-expiry`, `tui-websocket-reconnect` |
| Commit format | `<scope>: <subject>` — scope is a crate or area: `protocol:`, `auth:`, `persistence:`, `server:`, `tui:`, `web:`, `docs:`, `spike(...)`. Squash-merge to master via PR. |
| AI attribution | **Never.** No `Co-Authored-By`, no "Generated with", no `🤖`/AI credit markers in commits, PR bodies, comments, or docs — direct work or subagent work (Non-Negotiable Rule 3). |
| Spec storage | **Repo file, committed.** `docs/superpowers/specs/YYYY-MM-DD-<slug>-design.md` — never a tracker comment, never uncommitted. |
| Plan storage | **Repo file, committed.** `docs/superpowers/plans/YYYY-MM-DD-<slug>.md` — same rule. |
| Plan format | Goal / Architecture / Tech Stack / "**Spec:** … read it first" / Global Constraints / File Structure table / Task Order & Rationale / per-task `- [ ]` steps with failing-test-first TDD and a final format-and-commit step. The plan opens with the "For agentic workers: REQUIRED SUB-SKILL" note pointing at `superpowers:subagent-driven-development` / `executing-plans`. |
| Record-as-shipped | On completion, update the spec's `> **Status:**` to IMPLEMENTED, mark the plan's phases complete, and commit as `docs: record <…> as shipped` (e.g. `docs: record the device authorization grant as shipped`). |
| Architecture caveat | `ARCHITECTURE.md` describes the full intended design, including the engine core that does not exist yet (plan-first approval, Command/Event protocol, agent-to-agent bus, Slack/JIRA/GitHub integrations) — its "Current state" table marks what is built, and the latest `docs/superpowers/plans/` records where the build stands. Docs can still lag the code. Read the code. |
| Test command | `cargo test --workspace`. Per crate: `cargo test -p light-factory-<crate> [filter]`. The persistence integration test (`crates/persistence/tests/pg_store.rs`) reads `DATABASE_URL` and **skips** when PostgreSQL is unavailable — the suite is not fully offline; a local dev cluster is described in `ARCHITECTURE.md`. |
| Lint / format | `cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --all`. **Run `cargo fmt --all` before every Rust commit** (rustfmt is pinned in `rust-toolchain.toml`). |
| Frontend | `web/` is a Svelte 5 SPA (Vite), **not a Cargo crate** — `cargo build/test --workspace` must never require node. Web build: `cd web && npm run build`; deploy: `npm run deploy` (wrangler → Cloudflare Pages). |
| Known pre-existing failure | None known at the time of writing (2026-08). Do not treat a pre-existing failure as a regression you caused; if the suite was green on `master` before your branch, a new failure is yours. |
| Versioning | `rust-toolchain.toml` pins the toolchain (1.95.0); edition 2024. **Semver is a hard requirement (Non-Negotiable Rule 6):** wire-type and public-API changes are additive-only (semver-minor); a breaking change is semver-major — the 0.x minor-position bump while crates are pre-1.0 — and bumps the affected crate(s)' `version` in `Cargo.toml` within the same PR, documented in the spec. |

Full convention reference in `ARCHITECTURE.md` at the repo root. The above is the load-bearing subset for
this workflow.

## Load-Bearing Invariants (the "auth spine" — get these right)

These are not style rules; they are the invariants this repository is built around, and every review
step below checks them:

1. **Auth is fail-closed and never enumerates users.** Unknown email and invalid TOTP both surface
   as `InvalidCredentials` (`invalid_credentials`), so an attacker cannot distinguish "no such
   account" from "wrong code". Registration returns `EmailTaken` only after a real (TOTP-enabled)
   account is found. Never introduce a branch that reveals account existence.
2. **Registration is TOTP-first and passwordless.** The authenticator-app TOTP seed is the *sole*
   credential — there is no password. New accounts are created in a pending state and only become
   usable after the TOTP code is confirmed (`enable_totp`). Never reintroduce password columns or a
   passwordless fallback that skips TOTP.
3. **Secrets at rest are encrypted.** TOTP seeds are AES-256-GCM encrypted (base64 nonce‖ct) before
   they touch the database, under a master key from `LIGHT_SECRET_KEY` (base64, exactly 32 bytes).
   Startup **fails closed** without a valid key. A database leak alone must not yield usable seeds.
   Never store a seed (or any secret) in plaintext in the DB or in logs.
4. **Bearer tokens are returned in full but stored hashed.** Tokens are 32 random bytes (base64url);
   the database stores only the SHA-256 hex digest. `authenticate` hashes the presented token and
   looks it up; a session leak must not yield usable tokens. Never persist the raw token.
5. **Challenges and device grants are single-use by construction.** Registration challenges and
   RFC 8628 device grants are consumed atomically (`consume_challenge` / `consume_device_grant` are
   delete-and-return), so replay and concurrent-poll races cannot double-spend a code. The
   `device_code` is a high-entropy secret stored only as its hash; the `user_code` is a short,
   human-typed pairing code (low entropy by design — short-lived, single-use). Keep both properties.
6. **Secrets never cross the client boundary and are never logged.** `UserView` never carries the
   TOTP seed; the register response carries the seed only to the account owner at setup time, and
   never again. Passwords/TOTP codes/device codes/`LIGHT_SECRET_KEY` are never logged (tracing must
   not leak them), never echoed in error messages, never committed to the repo.
7. **Dependency flow is strictly inward.** `protocol` depends on nothing but serde/serde_json;
   `auth` defines the domain + `Store` trait seam and must never depend on a concrete persistence
   impl; `persistence` implements `Store` on top of `auth`; `server` wires auth+persistence into
   HTTP/WS; `tui` is a client that consumes `protocol` only. A new dependency edge that violates
   this is a design defect.
8. **The wire protocol is semver-constrained.** `protocol` wire types are typed serde records and
   externally-tagged enums (`ClientMessage`/`ServerMessage`: `Ping`/`Pong`/`Ready`/`Error`).
   Additions — new optional fields, new enum variants, new auth DTOs — are **semver-minor by this
   repo's convention**. Know the limit: an externally-tagged enum with no `#[serde(other)]` means an
   old receiver still fails to decode a *new variant* — additive is a versioning and
   source-compatibility guarantee, not wire forward-compatibility. Renaming, removing, or reordering
   a field or variant breaks the protocol and is semver-major: version bump in the PR + explicit
   review, never an incidental refactor side-effect (Non-Negotiable Rule 6).
9. **The product is human-in-the-loop — that is the architecture.** The engine core (not yet built;
   the next milestone, designed in `docs/superpowers/specs/2026-08-20-engine-core-design.md`) is
   plan-first approval: the agent proposes a structured plan, the human approves it once, and that
   approval authorizes the whole plan. Guardrails catch *deviation* from the approved scope, plus a
   sensitive-path floor that always asks. There is deliberately no per-tool risk-tier ladder.
   This is the *opposite* of otto's auto-apply model. Any edit-application path built later must be
   gated and fail-closed — never apply an edit without an explicit human approval. Nothing in this
   repo should silently widen an approval gate or bypass the human checkpoint.

## Tracker abstraction (GitHub Issues or ticketless)

light-factory uses **GitHub Issues when an issue exists, ticketless otherwise.** There is no JIRA.
Resolve once at intake and stay on that path for the whole task.

> **One-time bootstrap.** The `status:*` tracker labels below are NOT GitHub defaults. Create them
> once per repo before the first tracked issue (`gh label create status:in-progress status:in-review`),
> or the `--add-label` transitions will error ("could not find label").

| Lifecycle step | GitHub Issues | Ticketless |
|---|---|---|
| Ref form | `savvagent/light#123` (`#123` short) | the captured task brief |
| Intake / read AC | `gh issue view <n> --repo savvagent/light --json title,body,labels` | the user's instruction, captured verbatim in working memory + the PR body |
| → In Progress | `gh issue edit <n> --add-label status:in-progress` | n/a — capture `T_impl_start` only |
| → In Review | `gh issue edit <n> --add-label status:in-review` | n/a |
| Spec / Plan record | committed to `docs/superpowers/specs/` / `plans/`; reference the paths from the PR body | same — the repo docs ARE the durable record |
| Close | `gh issue close <n> --comment "…"` (the merged PR's `Closes #N` may have done it) | n/a — the Phase 6 summary is the close-out |
| Branch | `<slug>` (kebab) | `<slug>` |
| Commit subject | `<scope>: <subject>` | `<scope>: <subject>` |
| PR linkage | body contains `Closes #N` | body restates the task brief as the AC |

On the ticketless path there is no worklog sink, so the Phase 6 summary's timeline is the only time
record. `T_*` timestamps are still captured at phase boundaries to feed it.

## Phase 0 — Pre-flight (fresh context)

Intake reads ("what does this work want?") get corrupted by prior conversation cruft — stale paths,
abandoned plans, half-finished refactors.

**Two valid paths to fresh context:**

1. **Subagent dispatch** (default mid-conversation). Use the `Agent` tool with a self-contained
   prompt: issue key or task brief + "follow light-factory-development end-to-end" + any caller
   constraints. The subagent's context is fresh by construction; the parent session sees only the
   summary string. **When this path is used, see "Adaptation: when this skill runs inside a
   subagent" below** — interior reviewer dispatches collapse to named inline passes because a
   subagent cannot recursively dispatch.
2. **`/clear` + re-invoke** when staying in-session is preferable. **Preferred path when the highest
   review quality matters** — interior `Agent` dispatches work as designed only when this skill runs
   in the main thread.

**Not clean context:** hooks, memory entries, `additionalContext`, new files. Only a fresh process
or a fresh subagent qualifies.

**Branch + worktree safety:**

1. Run `git branch --show-current` in the current working directory.
2. **Trunk-sync check (mandatory).** Before any worktree creation, verify local trunk is in sync with
   origin:
   ```bash
   git fetch origin
   git rev-list origin/master..master            # MUST be empty
   ```
   If it returns commits, **local master is ahead of origin** — those commits will silently inherit
   into the new branch and contaminate the PR diff against `origin/master`. Surface them to the user
   (commits + their file paths); do NOT discard. They represent unpushed work that needs handoff
   BEFORE the new worktree is created.
3. If on `master` (or any trunk), create the worktree branching from `origin/master` explicitly (NOT
   local `master`) — belt-and-suspenders against step 2's check ever drifting:
   ```bash
   git worktree add .worktrees/<branch> -b <branch> origin/master
   ```
   Worktrees live inside the repo at `.worktrees/<branch>`. Never code on `master` directly.
4. If already on a feature branch in a worktree → proceed there.
5. `git status --porcelain` must be clean in the worktree before any code edit. Surface unexpected
   uncommitted changes; do NOT discard them.
6. **`master` is write-protected by policy.** The only way a commit reaches `master` is a reviewed,
   merged PR. Never commit to the main checkout's branch, never `git push origin master`, never merge
   a branch into `master` by hand. If the work needs to touch `master` (e.g. the record-as-shipped
   commit), it does so through its own worktree + PR like everything else.

Create TodoWrite todos for each phase (1–6) and check them off as you go.

## Adaptation: when this skill runs inside a subagent

Phase 0 lists subagent dispatch as a valid path to fresh context. But **a subagent's deferred-tool
set does NOT include the `Agent` tool** — a subagent cannot recursively dispatch further subagents.
This affects every interior reviewer/implementer dispatch (Phase 1 step 4, Phase 2 step 6, Phase 3
steps A/C/E/H, Phase 4 step 9).

When the orchestrator IS itself a subagent (Phase 0 path 1), apply these adaptations:

**Interior reviews run as named inline passes.** The orchestrator-subagent itself produces the spec
critique, plan critique, implementer report, spec-compliance report, and code-quality report — each
as a clearly delimited section written to the same prompt-template specification the `Agent` dispatch
would have used. The named output and acceptance criteria from `agent-prompts.md` still apply; only
the dispatch mechanism changes.

**Implementer "dispatch" collapses to direct execution.** The orchestrator-subagent is the
implementer. Apply the AUTONOMOUS MODE block to itself. Model-selection rules lose force — the
orchestrator runs at whatever model the parent dispatched it with.

**pr-review-toolkit dispatches are deferred to the parent.** The orchestrator-subagent cannot
dispatch `pr-review-toolkit:*` agents. It attaches the automated reviewer via `gh` CLI, then reports
the list of agents the parent must dispatch as a `Reviewers to dispatch from parent:` field in its
final report.

**The mandatory review trio (Phase 4 step 8) is also deferred to the parent.** A subagent cannot
dispatch `rust-pro` / `architect-reviewer` / `security-auditor`. The orchestrator-subagent MUST list
all three as required `Reviewers to dispatch from parent:` and MUST NOT merge — or declare the PR
mergeable — until the parent confirms all three reviews cleared. Non-Negotiable Rules 4–5 admit no
exception for the subagent path; deferring the trio is how they are honored there.

**Review-response subagent (Phase 4 step 9) is also deferred.** If the parent specified a halt point
at step 8, the orchestrator stops there cleanly. If invoked end-to-end with no parent halt point, it
MUST run the review-response work inline — same inline-pass discipline.

**Tradeoff.** Inline review by the same orchestrator that did the work loses the fresh-context
isolation that's the point of separate reviewer subagents. This is documented and acceptable, but not
equivalent. **For the highest-quality reviews, prefer Phase 0 path 2 (`/clear` + re-invoke in the
main thread).**

## Phase 1 — Intake + spec

> **Fast-path note:** Steps 1 + 2 always run. Steps 3 + 4 (spec draft + critique) are skipped if the
> work qualifies under "Fast-Path: Trivial Tasks." When fast-pathing, jump from step 2 directly to
> Phase 2 step 5 and write the minimal single-task plan.

### Step 1: Read the source directly

- **GitHub:** `gh issue view <n> --repo savvagent/light --json title,body,labels` — the body is the AC.
- **Ticketless:** capture the user's instruction verbatim in working memory. That string is the AC
  for the rest of the run; restate it in the PR body so the contract is durable.

If a teammate summarized it, still read the source — summaries lose AC.

### Step 2: Transition / mark In Progress

- **GitHub:** `gh issue edit <n> --repo savvagent/light --add-label status:in-progress`.
- **Ticketless:** nothing to transition.

**Capture `T_impl_start = now`** in ISO-8601 with explicit timezone offset. Hold for the Phase 6
summary timeline.

### Step 3: Spec draft (committed, no human review)

**Unlike general-development, the spec IS a repo file in this repository** — the plan-by-plan
convention requires it. Create `docs/superpowers/specs/YYYY-MM-DD-<slug>-design.md`, following the
existing specs' structure (read one first, e.g. the most recent design spec):

- Title: `# <Change> design` — descriptive, sentence case
- Status blockquote at top, e.g. `> **Status:** DRAFT — <one-line summary>` (updated to IMPLEMENTED at close-out)
- Optional `> **Implements:**`, `> **Depends on:**`, `> **Blocks:**` links when the change is a phase of a larger plan
- **Premise corrections** — if the task brief's premises do not survive contact with the repository
  (a very common occurrence here; the codebase and `ARCHITECTURE.md` are ahead of each other), record the
  corrections explicitly instead of silently building to the wrong premise
- **Scope** with **In:** and **Out:** — explicit non-goals
- Numbered sections (§1, §2, …) for each component: shape, configuration, security properties,
  testing. Cite `file:line` references to existing code where the design touches it

Required sections, wherever they fit: **Assumptions** (every choice made without asking, each with a
one-line rationale — the highest-value section), **Goal & Success Criteria** (one paragraph + 3–5
measurable bullets), **Error Handling & Edge Cases**, **Risks & Open Questions**.

**Commit the spec draft** as `docs: add <slug> design spec` before critique. The critique loop below
revises the *committed file* in place (new commits per round), never a working-memory-only copy.

### Step 4: Spec critique subagent

Dispatch using the **`Spec Critique`** template in [`agent-prompts.md`](agent-prompts.md) — **read
that file now and paste the template verbatim; do not improvise the prompt body.**
`subagent_type: general-purpose`. Fill `<PASTE FULL SPEC TEXT>` from the committed spec and cite the
source ref. In the Repo Profile placeholder, paste the "Load-Bearing Invariants" section above and
the relevant Repository Conventions.

**Maximum 2 revision rounds (3 reviewer dispatches total).** On Issues Found, revise the spec in the
committed file and redispatch with the updated text inline. If issues remain after the third pass,
append them to `Risks & Open Questions` and continue. Do NOT loop further.

**When the loop converges, commit the approved version** (once) and note the spec path in the plan
and the PR body.

## Phase 2 — Plan

> **Fast-path note:** On a fast-path ticket, write the minimal single-task plan directly — skip the
> critique loop.

### Step 5: Plan draft (committed, no human review)

Invoke `superpowers:writing-plans` via the Skill tool to load its current plan format. Then adapt it
to this repository's established plan format (read the most recent plan in
`docs/superpowers/plans/` first):

- Title: `# <Change> Implementation Plan`
- The opening "For agentic workers: REQUIRED SUB-SKILL" note pointing at
  `superpowers:subagent-driven-development` / `executing-plans` with the checkbox `- [ ]` convention
- **Goal**, **Architecture**, **Tech Stack** paragraphs
- **Spec:** line pointing at the committed design spec — "read it first. This plan implements it exactly."
- **Global Constraints** — the invariants that hold for every task (auth-spine items that apply,
  "no Claude/AI self-attribution", "run `cargo fmt --all` before every Rust commit", the inward
  dependency flow, etc.)
- **File Structure** table — `File | Responsibility`, each row prefixed **Create.**/**Modify.**
- **Task Order & Rationale** — why the tasks run in this order
- One `### Task N: <name>` per task, each with **Files:**, **Interfaces:** (consumes/produces), and
  `- [ ]` steps in **failing-test-first order**: write failing test → run → implement → run → commit.
  Include exact file paths, exact commands (`cargo test -p light-factory-<crate> <filter>`, etc.),
  and the expected result of each run.

Every task MUST include:
- Exact file paths in THIS repo's layout
- **Reminders for the out-of-band artifacts** from Phase 5 that the task touches (Fly image, Svelte
  bundle, DB migrations)
- **A semver step when the task makes a breaking change to a public interface or wire type:** a
  `- [ ]` step bumping the affected crate(s)' `version` in `Cargo.toml` (semver-major per
  Non-Negotiable Rule 6 — the 0.x minor-position bump at this repo's pre-1.0 state) before the
  final commit. Additive changes need no bump.
- The repo's actual test + lint commands for the TDD steps
- A final "Format and commit" step: `cargo fmt --all` + `git commit -m "<scope>: <subject>"`

**Commit the plan draft** as `docs: add <slug> implementation plan` before critique.

### Step 6: Plan critique subagent

Dispatch using the **`Plan Critique`** template in [`agent-prompts.md`](agent-prompts.md) — **read
that file now and paste the template verbatim.** `subagent_type: general-purpose`. Fill `<PASTE FULL
PLAN TEXT>`, `<PASTE FULL SPEC TEXT>`, and the Repo Profile (Load-Bearing Invariants + conventions +
test/lint commands).

Same revision-loop shape as step 4 (revise the committed file, redispatch with the updated text
inline). **Maximum 2 revision rounds.** If unresolved issues remain, prepend a `## Known Plan Gaps`
section and continue.

**When the loop converges, commit the approved version** (once).

## Phase 3 — Implement

Per `superpowers:subagent-driven-development`: read the plan ONCE, extract every task's full text +
context into your own working memory. Create one TodoWrite entry per task. **Do NOT make implementer
subagents re-read the plan** — provide them the full task text inline.

**Sequential, not parallel.** Implementer subagents on the same branch will conflict on the working
tree. Parallelism happens across separate worktrees on separate features, not within one run.

For each task in plan order:

### A. Dispatch implementer

Dispatch using the **`Implementer Dispatch`** template in [`agent-prompts.md`](agent-prompts.md) —
**read that file now and paste the template verbatim** (the `## AUTONOMOUS MODE` block and `## Report
Format` are load-bearing). `subagent_type: general-purpose`. Fill the task text, the Context block
(include the relevant Load-Bearing Invariants + the repo's test/lint commands), the source ref, and
the worktree path.

**Model selection:**
- Mechanical 1–2-file tasks with complete specs → `model: "haiku"`
- Multi-file integration work → omit (inherit parent)
- Design-judgment tasks the plan explicitly flags → `model: "opus"`

### B. Handle implementer status

| Status | Action |
|---|---|
| `DONE` | Proceed to spec compliance review (step C) |
| `DONE_WITH_CONCERNS` | If correctness/scope: dispatch fix subagent now with the specific concern as the new task. If minor: log in per-task ledger and proceed |
| `NEEDS_CONTEXT` | If discoverable in the repo, re-dispatch with the context filled in. If genuinely unknowable: treat as BLOCKED |
| `BLOCKED` | See Stop & Escalate below |

### C. Spec compliance review

Dispatch using the **`Spec Compliance Review`** template in [`agent-prompts.md`](agent-prompts.md) —
**read that file now and paste the template verbatim** (the `## CRITICAL: Do Not Trust The Report`
block is load-bearing — the reviewer must read the actual commits, not the implementer's claims).
`subagent_type: general-purpose`. Fill the task text and the implementer's report verbatim.

### D. Spec fix loop (max 2 fix dispatches)

- ✅ → quality review (step E).
- ❌ → re-dispatch implementer with status `FIX_SPEC_ISSUES`, supplying the reviewer's findings as
  the new task. Re-run spec review.
- **Three failed spec reviews in a row → escalate.**

### E. Code quality review

Capture commit boundaries:
- `BASE_SHA = git rev-parse HEAD~<N>` where N = commits this task produced
- `HEAD_SHA = git rev-parse HEAD`

Dispatch using the **`Code Quality Review`** template in [`agent-prompts.md`](agent-prompts.md) —
**read that file now and paste the template verbatim.** `subagent_type: code-reviewer` (NOT
general-purpose). Fill `<BASE_SHA>`/`<HEAD_SHA>` and the task text.

### F. Quality fix loop (max 2 fix dispatches)

- No Critical/Important → mark task complete in TodoWrite; record any Minor issues in per-task ledger.
- Critical/Important → re-dispatch fixer with those specific findings. Re-run quality review.
- **Three failed quality reviews in a row → escalate.**

**Pragmatism rule:** Minor / optional suggestions ≠ blockers. Treat code-quality "Approved with
suggestions" as DONE; do not auto-dispatch fix loops for non-blocking suggestions.

### G. Per-task ledger

Maintain an internal running record per task: name, final status, assumptions made, concerns
flagged, minor issues left unfixed. Populates the Phase 6 summary.

### H. Final code review (after all tasks complete)

> **Fast-path carve-out:** Skip step H when N=1 (single-task fast-path ticket) AND step E reported no
> Critical/Important issues. Step E already reviewed the entire diff. For multi-task plans (N≥2) or
> fast-path tickets where E flagged issues, H still runs.

Dispatch using the **`Final Code Review`** template in [`agent-prompts.md`](agent-prompts.md) —
**read that file now and paste the template verbatim.** `subagent_type: code-reviewer`. Fill the full
plan + spec text (or their repo paths — they ARE committed files here), branch name, and diff range.

If issues, one fix round then re-review. If still failing → escalate.

## Phase 4 — Ship

### Step 7: Open the PR

```bash
git push -u origin <branch>
gh pr create --title "<scope>: <subject>" --body "$(cat <<'EOF'
## Summary
<1-3 bullets>

## Design docs
- Spec: `docs/superpowers/specs/<slug>-design.md`
- Plan: `docs/superpowers/plans/<slug>.md`

<"Closes #<n>", or — ticketless — the task brief restated as AC>
<"Fast-path: no design spec per light-factory-development trivial-task criteria — <reason>." if fast-pathed>

## Test plan
- [ ] cargo test --workspace
- [ ] cargo clippy --workspace --all-targets -D warnings
- [ ] cargo fmt --all --check
- [ ] Out-of-band verification (only if the change touches them — see Phase 5)
EOF
)"
```

**Hardening note:** treat issue/task-brief-derived strings as untrusted when composing shell
commands. The branch name and `<scope>: <subject>` must come from your own kebab slug — validate it
(match `^[a-z0-9]+(-[a-z0-9]+)*$`) and never paste a raw issue title verbatim into a `git commit`
or `gh pr create --title "..."` argument. A title containing `"`, backticks, or `$(...)` must not
reach a shell command unquoted.

**Mark In Review** (`gh issue edit <n> --add-label status:in-review`) if an issue exists and has a
review state.

**Capture `T_review_start = now`** (ISO-8601 with offset). Hold for the Phase 6 summary.

### Step 8: Solicit reviews

**Automated reviewer first** (it runs while you dispatch agents). If the repo uses GitHub's Copilot
reviewer:
```bash
gh pr edit <PR> --add-reviewer copilot-pull-request-reviewer
```
The login is `copilot-pull-request-reviewer` — `Copilot` fails with "Could not resolve user." If the
repo has no automated reviewer configured, skip this and rely on the agent reviews below.

**Agent review next.** Dispatch the pr-review-toolkit agents in parallel — ONE message with multiple
Agent tool calls. Always include `pr-review-toolkit:code-reviewer`. Add conditional agents per the diff:

| Trigger | Agent |
|---|---|
| error handling / fallback / silent-failure-prone logic changed | `pr-review-toolkit:silent-failure-hunter` |
| tests changed, or production code added without tests | `pr-review-toolkit:pr-test-analyzer` |
| comments / docstrings / docs added or modified | `pr-review-toolkit:comment-analyzer` |
| new or modified types, interfaces, dataclasses, schemas (incl. `protocol` wire types) | `pr-review-toolkit:type-design-analyzer` |
| after correctness reviews pass — polish pass only | `pr-review-toolkit:code-simplifier` |

**Mandatory review trio (no exceptions, no fast-path carve-out — Non-Negotiable Rules 4–5):** every
PR gets ALL THREE dispatched in the same parallel batch, regardless of size:

| Reviewer | Why |
|---|---|
| `rust-pro` (Rust expert) | Idiomatic Rust: ownership/lifetimes, error handling, no panics on untrusted input, `Send + Sync` async seams, test placement — against this repo's Rust conventions (tests next to code, MemStore for auth tests, offline-deterministic where possible). |
| `architect-reviewer` (architect) | Architectural consistency: inward dependency flow (`protocol` → `auth` → `persistence` → `server` → `tui`), crate boundaries, trait-seam design, whether the change follows or erodes the documented architecture (`ARCHITECTURE.md` + latest plan), spec/plan alignment. |
| `security-auditor` (independent) | Security of the actual diff. **Receives ONLY the diff** — never the spec/plan/brief/PR-body summary (Non-Negotiable Rule 5). See the Independent Security Review template in `agent-prompts.md`. |

The rust-pro and architect-reviewer reviews the actual diff (commit range), not the summary. Treat
their findings like any other review: Critical/Important must be fixed or explicitly dismissed before
merge; all three must clear before merge (step 11).

Add ad-hoc domain agents (`frontend-developer`) on top by topic if the change warrants it. The
independent `security-auditor` pass above already covers every PR; for changes touching the auth
spine (register/login/device grant/secret handling/WS auth), give it the extra instruction to
scrutinize those specific invariants hardest.

Aggregate the agent reports into a single PR comment grouped by **Critical / Important / Suggestions
/ Strengths** so review threads stay flat instead of one comment per agent.

### Step 9: Fork a review-response subagent

Once reviews have posted, immediately dispatch a dedicated review-response subagent with fresh
context to avoid polluting the main thread with review-fix churn.

Dispatch using the **`Review-Response Subagent`** template in [`agent-prompts.md`](agent-prompts.md)
— **read that file now and paste the template verbatim** (the fix-or-dismiss + thread-resolve
mutation + inline-reply requirements are load-bearing). `subagent_type: general-purpose`. Fill the PR
number `<N>` and the source ref.

### Step 10: PR review loop

Human reviewers post on their own schedule. Each new round forks a NEW review-response subagent.
Continue until merged.

**Idempotency required.** Every iteration must be safe to no-op. First action of any loop iteration:
enumerate unresolved threads:

```bash
gh pr view <PR> --json reviewDecision,reviews,statusCheckRollup
gh api repos/savvagent/light/pulls/<PR>/comments
# GraphQL: pullRequest.reviewThreads(first: 100) { nodes { id isResolved comments { ... } } }
```

A comment is "new since last pass" if its thread is unresolved AND your last reply (if any) is older
than the latest comment in that thread. If zero new, exit cleanly — no commits, no replies, no
tracker writes.

> **The merge gate is the test suite on YOUR merge commit.** This repo has no CI (no `.github/`), so
> "green" means a manual `cargo test --workspace` (+ clippy/fmt) on the merge commit — see Phase 5
> step 13. If CI is ever added, the gate becomes the CI run your merge commit triggered (by run ID,
> never "the latest run").

| Iteration state | Action |
|---|---|
| All threads resolved + merge-commit test run green + approval present | Merge (step 11) |
| All threads resolved + merge-commit test run green + awaiting human approval | Idle. Next iteration no-ops |
| Open threads you cannot address (security / missing requirement / ambiguous) | Escalate per Stop & Escalate |
| Same unresolved thread across multiple iterations after you replied | Wake the human reviewer. Do NOT silently retry |
| Test run failed on the merge commit | Treat as "fix this" comment. Address before next iteration |

### Step 11: Merge

**Run `gh pr merge` from the main checkout, NOT from inside the worktree.** From-worktree merge can
corrupt main-checkout staging.

```bash
cd <main-checkout-root>
gh pr merge <PR> --squash --delete-branch    # or --merge per repo convention
```

**Capture `T_pipeline_start = now`** (ISO-8601 with offset).

### Step 12: Clean up + record-as-shipped

```bash
cd <main-checkout-root>
git checkout master && git pull --ff-only
git branch -d <branch>                        # -D only if force needed and intentional
git worktree remove .worktrees/<branch>       # required — worktrees live inside the repo
```

**Record-as-shipped (mandatory, do not skip):** in a fresh worktree off the updated `master` — the
same `.worktrees/<branch>` + PR flow as the feature itself, never a direct commit to the main
checkout — update the spec's `> **Status:**` to `IMPLEMENTED` and mark the plan's completed phases,
then commit as `docs: record <…> as shipped` and merge via PR. This is how the plan-by-plan record
stays current — the plan repository IS the project's history.

## Phase 5 — Deploy, verify, close

**There is no deploy pipeline, no CI, and no staging/prod automation.** light-factory's backend is an
Axum server deployed to Fly.io (`Dockerfile` + `fly.toml`), its frontend is a Svelte SPA deployed to
Cloudflare Pages (`web/`), and its data layer is PostgreSQL (fly Managed Postgres) via sqlx
migrations. Phase 5 collapses to: run the test suite on the merge commit, run the out-of-band
verification checklist for what the change touched, then close.

### Step 13: Confirm YOUR merge commit is green

There is no CI in this repo, so the check is a manual `cargo test --workspace` (+
`cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --all --check`) on the merge
commit. If CI is ever added, watch the run YOUR merge commit triggered by run ID — do NOT infer
status from "the latest run", which a teammate's merge seconds after yours can steal.

**Capture `T_verify_start = now`.**

### Step 14: Out-of-band artifact verification

The test suite does NOT auto-apply these. Verify whatever the change touched, explicitly:

- **Backend Fly image** — if `Dockerfile` or `fly.toml` changed: build it and exercise the changed
  surface. Note: **no docker on this machine; podman is available** (`podman build -t light-factory .`).
  Confirm the binary serves `/health` and the auth/WS surface still works.
- **Frontend SPA bundle** — if `web/` changed: run `cd web && npm run build` (the Vite build is the
  check) and confirm the deploy target is sane. If `web/package.json`'s `deploy` script or
  `web/.env.production` changed, verify the Cloudflare Pages wiring (`wrangler pages deploy dist`)
  and that `VITE_API_URL` points at the right backend origin.
- **Database migrations** — if `crates/persistence/migrations/` changed: run
  `cargo test -p light-factory-persistence` against a local PostgreSQL (needs `DATABASE_URL`; the dev
  cluster is described in `ARCHITECTURE.md` — `pg_ctl -D .pgdata …`). Migrations are applied by
  `run_migrations` at server startup; new migrations must be additive (never rewrite an already-shipped
  migration) and must not break a fresh-cluster apply.
- **CI** — if `.github/` changed (none exists yet): confirm the workflow files are valid and the jobs
  actually run.

A vacuously-satisfied item ("no deploy/distribution change in this PR") is satisfied, not skipped —
state it explicitly.

### Step 15: Production / target verification

The target is the local workspace + the master branch's own test suite. For a change that shipped,
the smoke is: `cargo test --workspace` green on the merge commit, plus `cargo clippy --workspace
--all-targets -D warnings` and `cargo fmt --all --check` clean, plus the step-14 out-of-band items.
For user-facing surfaces (auth routes, WS, TUI), run the server binary once
(`cargo run -p light-factory-server`) and exercise the changed surface; run the TUI
(`cargo run -p light-factory-tui`) for client-side changes.

### Step 16: Close

- **GitHub:** `gh issue close <n> --repo savvagent/light --comment "<summary>"` (the merged PR's
  `Closes #N` may have closed it already — verify with `gh issue view <n> --json state`).
- **Ticketless:** no close action — the Phase 6 summary is the close-out.

Close-out summary:
```
Shipped.

PR: <url>
Out-of-band applied: <fly-image/web-bundle/migrations, or "none">
Smoke: <one-line outcome>
```

## Phase 6 — Final summary

Output a single concise message:

```
light-factory-development complete.

Source: <issue #> / task brief — <title> — Closed
PR: <url>
Branch: <branch> (deleted, worktree removed)
Spec: docs/superpowers/specs/<slug>-design.md
Plan: docs/superpowers/plans/<slug>.md
Tasks completed: N / N
Commits: <count>
Timeline: <T_impl_start → T_review_start → T_pipeline_start → T_verify_start, or "n/a">

Out-of-band applied: <list, or "none">

Assumptions worth reviewing (from spec + per-task ledger):
- <bullet>
(up to 5)

Minor issues left unaddressed (intentional, low-priority):
- <bullet, or "none">

Final reviewer assessment: <Ready / Needs follow-up — details>
```

Then STOP. Do not pick up the next task. Do not offer to chain another run.

## Stop & Escalate

Stop the pipeline and return control to the developer when ANY of these is true:

1. A task is BLOCKED and re-dispatching with more context did not unblock it after one retry.
2. A task fails spec review three times in a row.
3. A task fails quality review three times in a row (with Critical or Important issues).
4. Test infrastructure is broken in a way that prevents verifying any task.
5. The plan has internal inconsistencies (a later task assumes a structure earlier tasks didn't produce).
6. The pipeline has run for unreasonable wall-clock time and is making no progress.
7. The AC contradicts the spec/plan you built (mid-flight requirements change).
8. An agent review surfaces a security finding (auth, injection, secrets, PII) — especially one
   touching registration/login, the device grant, secret-at-rest encryption, token hashing, or WS auth.
9. A proposed change would widen a fail-closed path, leak a secret into logs/responses, store a token
   in plaintext, break single-use semantics of a challenge/grant, or violate the inward dependency
   flow — these are not negotiable design choices.
10. Out-of-band verification (Phase 5 step 14) fails after a green merge.
11. The same bug pattern is discovered elsewhere — file a follow-up issue; do NOT silently widen scope.
12. An out-of-band prerequisite from another change (an unmerged plan, an unbuilt image, an unshipped
    migration) is missing.

On escalation, output:

```
light-factory-development halted at Phase <N> — <step name>.

Reason: <one of the conditions above, with specifics>
Source: <issue/brief>
Branch: <branch>
Worktree: .worktrees/<branch>
PR: <url, if open>
Last successful step: <step name>
Commits so far: <git log --oneline since branch point>
Recommended next step: <suggestion>
```

Then STOP. Do not push, do not open a PR, do not merge, do not close.

Speed pressure does not eliminate any step. It can require escalation; it never authorizes skipping.

## Calibration vs. Skipping

Within each step you may calibrate effort to risk. You may NEVER eliminate a step.

| Step | Cheapest valid form for a small change | Skip? |
|---|---|---|
| Convention reference | Skim ARCHITECTURE.md + the latest plan + test command | NEVER |
| Read source | 20-second skim of description + AC | NEVER |
| Spec draft | 1-page spec with Assumptions + Brief + Scope (committed) | **Only** if fast-path criteria met |
| Spec critique | 1 reviewer dispatch | **Only** when the spec is skipped under fast-path |
| Plan draft | Minimal single-task plan (committed) | NEVER — house style keeps the plan even on fast-path |
| Plan critique | 1 reviewer dispatch | **Only** when the plan is fast-pathed (trivial task) |
| Worktree at start | `git worktree add .worktrees/<branch> -b <branch> origin/master` | NEVER (never code on master) |
| Master-branch writes | reviewed, merged PR (squash) — incl. record-as-shipped | NEVER (no direct push/hand-merge) |
| Implementer dispatch | 1 Agent call with full task text inline | NEVER |
| Spec compliance review | 1 reviewer dispatch reading actual commits | NEVER |
| Quality review | 1 code-reviewer Agent dispatch | NEVER |
| Automated reviewer | 1 `gh pr edit --add-reviewer …` | Only if the repo has none configured |
| Rust expert + architect + independent security review | 1 parallel Agent dispatch each (`rust-pro`, `architect-reviewer`, blind-diff `security-auditor`) | NEVER |
| pr-review-toolkit agents | 1 parallel Agent dispatch | NEVER |
| Review-response subagent | 1 Agent dispatch with PR# + ref | NEVER |
| Branch + worktree cleanup | `git branch -d` + `git worktree remove` | NEVER |
| Record-as-shipped | update spec Status + mark plan phases; `docs:` commit | NEVER |
| Out-of-band verification (#14) | 30-second check per touched surface | NEVER (vacuous is fine) |
| Target verification (#15) | `cargo test --workspace` + clippy + fmt on the merge commit | NEVER |
| Tracker transitions | 1 call per transition | Only on the ticketless path |

A vacuously-satisfied step ("no out-of-band surface touched") is satisfied, not skipped. State it
explicitly.

## Common Rationalizations (All Are Violations)

| Excuse | Reality |
|---|---|
| "It's a small change, skip the spec/plan" | Only skip the *spec* if ALL fast-path criteria hold. The plan doc is not skippable. |
| "I'll ask the user mid-run about an ambiguity" | Fully autonomous. Make the most reasonable interpretation, document under Assumptions, continue. |
| "Slack/the PR/the summary IS the spec" | Summaries lose AC. The brief/issue is source of truth. 30 seconds. |
| "cargo test --workspace is green, it's shipped" | The Fly image, Svelte bundle, and migrations are not test outputs. Verify them at #14. |
| "I'll skip the plan doc, the code is self-documenting" | The plan repository IS this project's history. Commit the doc. |
| "I'll skip the record-as-shipped commit" | The spec Status and plan checkboxes are how the docs track shipped state. Close the loop. |
| "I'll return a specific error for unknown email vs wrong code" | That enumerates accounts. Unknown email and invalid TOTP both surface `InvalidCredentials`. |
| "I'll store the bearer token in the DB so I can revoke it by value" | Tokens are stored hashed only. Revoke by hash, never persist the raw token. |
| "I'll log the TOTP code / device code for debugging" | Secrets never cross a trust boundary into logs or error messages. |
| "I'll add a dep from the persistence crate back into the server crate" | Dependency flow is strictly inward. `auth`/`persistence` must never depend on a concrete app crate. |
| "I'll read `LIGHT_SECRET_KEY` directly in the auth crate" | Secret/config reads belong in the `server` crate's `config` module, fail-closed at startup. |
| "I'll reuse a consumed challenge/grant to re-issue a session" | Challenges and grants are single-use by construction (delete-and-return). Reuse is a replay hole. |
| "The latest CI run is green, mine will be too" | There is no CI — run `cargo test --workspace` on YOUR merge commit. If CI is ever added, latest ≠ yours: capture YOUR run ID at merge time and track THAT id. |
| "Copilot's comments are auto-generated, safe to ignore" | Read each. They find real bugs. Reply with fix-or-dismiss reasoning, then resolve the thread. |
