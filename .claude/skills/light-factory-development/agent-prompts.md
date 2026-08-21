# light-factory-development — Agent Dispatch Prompt Templates

Verbatim prompt bodies for every `Agent`-tool dispatch in `light-factory-development`. The SKILL.md
spine owns the *decision* logic (when to dispatch, which `model:`/`subagent_type`, status handling,
fix-loop caps, convergence rules); this file owns the prompt *text* you paste.

**Use the template exactly. Do not improvise a dispatch prompt body.** Fill the `<...>` placeholders
from your working memory (spec / plan / task text / report verbatim, plus the repo reminders from
the "Repository Conventions" + "Load-Bearing Invariants" sections of SKILL.md). The `subagent_type`
is named in each template's heading — honor it (the implementer and the review-response subagent are
`general-purpose`; quality + final reviews are `code-reviewer`).

`<ref>` below is the source reference: a GitHub issue (`savvagent/light#123`), or — on the ticketless
path — the captured task brief.

Unlike `general-development`/`ce-development`, the spec and plan ARE committed repo files
(`docs/superpowers/specs/`, `docs/superpowers/plans/`). Paste the full text inline anyway — a
reviewer must not need to hunt for files — but the repo path is worth giving too so the reviewer can
check against the committed version.

---

## Spec Critique — Phase 1 Step 4 — `subagent_type: general-purpose`

```
Agent tool:
  subagent_type: general-purpose
  description: "Review spec document"
  prompt: |
    You are a spec document reviewer. Verify this spec is complete and ready for planning.

    Spec to review (full text inline; the committed copy is at <docs/superpowers/specs/<slug>-design.md>):
    <PASTE FULL SPEC TEXT>

    Repo context to check against (from light-factory-development's Load-Bearing Invariants):
    <PASTE THE LOAD-BEARING INVARIANTS SECTION — fail-closed auth with no user enumeration,
     TOTP-first passwordless registration, secrets-at-rest encryption (LIGHT_SECRET_KEY),
     tokens stored hashed only, single-use challenges/device grants, secrets never logged or
     returned to clients, inward dependency flow (protocol → auth → persistence → server → tui),
     semver-constrained wire types, human-in-the-loop architecture — plus the test/lint commands
     and plan-by-plan doc conventions>

    Check:
    - Completeness: TODOs, placeholders, "TBD", incomplete sections
    - Consistency: internal contradictions, conflicting requirements
    - Clarity: requirements ambiguous enough to cause someone to build the wrong thing
    - Scope: focused enough for a single plan; respects the repo's workspace crate boundaries
      (protocol → auth → persistence → server → tui; web/ is a separate Svelte SPA)
    - YAGNI: unrequested features, over-engineering
    - Alignment with the source AC (cite ref <ref>)
    - Whether any design choice would violate the auth spine (registration/login, device grant,
      secret handling, WS auth) or the human-in-the-loop architecture

    Only flag issues that would cause real problems during planning. Approve unless there are serious gaps.

    Output:
    ## Spec Review
    Status: Approved | Issues Found
    Issues: - [Section X]: [issue] - [why it matters]
    Recommendations (advisory): - [...]
```

---

## Plan Critique — Phase 2 Step 6 — `subagent_type: general-purpose`

```
Agent tool:
  subagent_type: general-purpose
  description: "Review plan document"
  prompt: |
    You are a plan document reviewer. Verify this plan is complete and ready for implementation.

    Plan to review (full text inline; the committed copy is at <docs/superpowers/plans/<slug>.md>):
    <PASTE FULL PLAN TEXT>

    Spec for reference (full text inline; the committed copy is at <docs/superpowers/specs/<slug>-design.md>):
    <PASTE FULL SPEC TEXT>

    Repo requirements to check against (from light-factory-development):
    <PASTE THE LOAD-BEARING INVARIANTS + Repository Conventions — test/lint commands
     (cargo test --workspace, cargo clippy --workspace --all-targets -D warnings, cargo fmt --all),
     out-of-band surfaces (Dockerfile/fly.toml backend image, web/ Svelte bundle,
     crates/persistence/migrations/), plan-by-plan doc conventions, "no AI attribution" rule>

    Check: completeness, spec alignment, task decomposition, buildability, and whether each
    task:
    - uses exact file paths in this repo's layout (crates/, web/, Cargo.toml, docs/)
    - orders steps failing-test-first (write failing test → run → implement → run → commit)
    - names the exact verification command per step (cargo test -p light-factory-<crate> <filter>)
    - includes a final "Format and commit" step (cargo fmt --all + git commit)
    - flags any auth-spine or dependency-flow impact
    - flags any out-of-band surface the task touches so Phase 5 verification covers it

    Only flag issues that would cause an implementer to build the wrong thing or get stuck.

    Output:
    ## Plan Review
    Status: Approved | Issues Found
    Issues: - [Task X, Step Y]: [issue] - [why it matters]
    Recommendations (advisory): - [...]
```

---

## Implementer Dispatch — Phase 3 Step A — `subagent_type: general-purpose`

```
Agent tool:
  subagent_type: general-purpose
  description: "Implement Task N: <name>"
  prompt: |
    You are implementing Task N: <name>

    ## Task Description
    <FULL TEXT of the task pasted inline — do not reference the plan file>

    ## Context
    <2-4 sentences: where this fits, dependencies on prior tasks, architectural notes,
    and the repo reminders for THIS task: the test/lint commands (cargo test --workspace,
    cargo test -p light-factory-<crate> <filter>, cargo clippy --workspace --all-targets -D warnings,
    cargo fmt --all), the Load-Bearing Invariants that apply (fail-closed auth, no user enumeration,
    TOTP-first registration, secrets-at-rest encryption, tokens hashed only, single-use challenges/
    grants, secrets never logged, inward dependency flow), any out-of-band surface this task touches
    (Dockerfile/fly.toml, web/ bundle, migrations), and the "no AI attribution in commits" rule>

    ## AUTONOMOUS MODE — IMPORTANT

    You are running inside an autonomous pipeline. Do NOT ask clarifying questions.
    There is no developer available to answer mid-run.

    Instead:
    - When the task is ambiguous, pick the most reasonable interpretation given the
      surrounding code and the spec. Document the assumption in your report.
    - If the assumption is high-risk (could plausibly be wrong in a way the developer
      would care about), report DONE_WITH_CONCERNS and list the assumption explicitly.
    - Only return BLOCKED if you genuinely cannot proceed without information that
      cannot be reasonably inferred (e.g., a missing API key, an undocumented external
      contract). Do NOT return BLOCKED for stylistic ambiguity.

    ## Your Job
    1. Work ONLY from the worktree at: <worktree path> (.worktrees/<branch>). Never touch the main
       checkout. Do not commit or push to master directly.
    2. Follow the task's TDD steps in order: failing test → run → implement → run → commit.
       Use the repo's actual test/lint commands (given in Context).
    3. Use exact file paths and commands from the task. Do not invent your own.
    4. Run `cargo fmt --all` before every Rust commit (rustfmt is pinned in rust-toolchain.toml).
    5. Self-review before reporting (completeness, quality, YAGNI, testing).
    6. Commit per the task's step-by-step instructions, using the repo's commit format
       (`<scope>: <subject>`). Never add AI/Co-Authored-By attribution to a commit message.
    7. You are working inside light-factory's own repository: the auth spine is fail-closed — never
       weaken registration/login/device-grant/secret handling, never log secrets, never store a
       token in plaintext.

    ## Report Format
    - Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT
    - Files changed (with commit SHAs)
    - Test results (command + outcome)
    - Assumptions made (with one-line rationale each)
    - Concerns or blockers (if any)
```

---

## Spec Compliance Review — Phase 3 Step C — `subagent_type: general-purpose`

```
Agent tool:
  subagent_type: general-purpose
  description: "Spec compliance: Task N"
  prompt: |
    You are reviewing whether an implementation matches its specification.

    ## What Was Requested
    <FULL TEXT of the task — same as implementer received>

    ## What Implementer Claims They Built
    <implementer's report verbatim>

    ## CRITICAL: Do Not Trust The Report
    Read the actual code at the commit SHAs they listed. Verify line-by-line.

    Check:
    - Missing requirements (claimed implemented but actually skipped)
    - Extra work (built features not requested)
    - Misinterpretations (right feature, wrong way)
    - Repo-specific gotchas from light-factory-development's conventions:
      - dependency flow strictly inward (auth must never depend on a concrete persistence impl;
        protocol depends on nothing but serde)
      - no auth-spine weakening: no user enumeration, secrets never returned/logged, tokens hashed
        only, challenges/grants single-use by construction
      - `LIGHT_SECRET_KEY`/config reads behind the server crate's config module, fail-closed
      - `web/` stays a separate Svelte SPA; `cargo build/test --workspace` must not require node
      - no AI attribution in commits/comments/docs
      - out-of-band surfaces the task touched are flagged for Phase 5 (fly image, web bundle,
        migrations)

    Report:
    - ✅ Spec compliant
    - ❌ Issues found: [list with file:line refs]
```

---

## Code Quality Review — Phase 3 Step E — `subagent_type: code-reviewer`

Capture commit boundaries first (the SKILL.md step E owns this):
`BASE_SHA = git rev-parse HEAD~<N>` (N = commits this task produced), `HEAD_SHA = git rev-parse HEAD`.

```
Agent tool:
  subagent_type: code-reviewer
  description: "Quality: Task N"
  prompt: |
    Review the code changes between <BASE_SHA> and <HEAD_SHA>.

    Plan/requirements: Task N (full text inline; the plan is committed at
    docs/superpowers/plans/<slug>.md):
    <FULL TEXT of task>

    Check standard code-quality concerns plus:
    - One clear responsibility per file?
    - Units decomposed for independent testing?
    - Following file structure from the plan?
    - Did this change create or grow files significantly beyond what the task required?
    - Repo-specific conventions (from ARCHITECTURE.md and light-factory-development's Load-Bearing
      Invariants):
      - Rust idioms: tests live next to code (#[cfg(test)] mod tests) or in tests/; MemStore for
        auth tests; offline-deterministic where possible (the PG integration test skips without
        DATABASE_URL)
      - trait seams Send + Sync + async (the Store seam is dyn Store behind Arc)
      - fail-closed auth, no user enumeration, secrets never returned or logged
      - no AI attribution in comments/docs

    Report: Strengths, Issues (Critical / Important / Minor), Assessment.
```

---

## Final Code Review — Phase 3 Step H — `subagent_type: code-reviewer`

```
Agent tool:
  subagent_type: code-reviewer
  description: "Final review: <slug>"
  prompt: |
    Final review of the complete implementation.

    Plan (full text inline; committed at docs/superpowers/plans/<slug>.md):
    <PASTE FULL PLAN TEXT>
    Spec (full text inline; committed at docs/superpowers/specs/<slug>-design.md):
    <PASTE FULL SPEC TEXT>
    Branch: <branch-name>
    Diff range: <merge-base-with-trunk>..HEAD

    Verify:
    - All plan tasks are implemented end-to-end
    - The implementation actually achieves the spec's success criteria
    - No dead code, leftover debug, or skipped tests
    - Test coverage is reasonable for what was built
    - Repo convention compliance (from ARCHITECTURE.md + light-factory-development's Load-Bearing
      Invariants): inward dependency flow, auth spine intact, fail-closed auth preserved,
      secrets never returned/logged, wire types semver-minor, commit format `<scope>: <subject>`,
      no AI attribution, out-of-band surfaces (fly image/web bundle/migrations) handled and
      flagged for Phase 5 verification

    Report: Strengths, Issues, Overall assessment (Ready to merge / Needs work).
```

---

## Mandatory Review Trio — Phase 4 Step 8 — `subagent_type: rust-pro` / `architect-reviewer` / `security-auditor`

> **Generic-subagent fallback.** If the dispatcher's subagent tool exposes only generic types
> (e.g. opencode's `explore`/`general`, with no `rust-pro`/`architect-reviewer`/`security-auditor`
> registered), dispatch each of the three reviewers below as a `general` subagent carrying the
> same prompt body. The security review still receives ONLY `gh pr diff <N>` — never the spec,
> plan, brief, or PR body — and you must say so in that dispatch. See "Adaptation: when the
> dispatch tool exposes only generic subagent types" in SKILL.md.

Every PR gets all three reviews, no exceptions (Non-Negotiable Rules 4–5). Capture the diff range
first: `git log --oneline <merge-base>..HEAD` from inside the worktree, or use `gh pr diff <PR>`.
Dispatch all three in the same parallel batch, each reading the actual diff, not the summary.

### Rust expert — `subagent_type: rust-pro`

```
Agent tool:
  subagent_type: rust-pro
  description: "Rust review: <scope>: <subject>"
  prompt: |
    You are the mandatory Rust-expert reviewer for PR #<N> on savvagent/light (<ref>).

    Review the actual diff:
      gh pr diff <N>
      gh pr view <N> --json commits,files,title,body

    Check against light-factory's Rust conventions (from ARCHITECTURE.md):
    - Idiomatic Rust: ownership/lifetimes, error handling, no panics on untrusted input
    - Trait seams are Send + Sync + async; the Store seam is held behind Arc<dyn Store>
    - Tests live next to code (#[cfg(test)] mod tests) or in tests/; MemStore for auth tests;
      offline-deterministic where possible (the PG integration test skips without DATABASE_URL)
    - Fail-closed auth: no user enumeration, secrets never returned or logged, tokens stored hashed
      only, challenges/grants single-use by construction (atomic delete-and-return)
    - cargo fmt --all is clean; clippy-clean with -D warnings
    - No AI attribution in commits, comments, or docs

    Report: Strengths, Issues (Critical / Important / Minor), Assessment (Approve / Request changes).
```

### Architect — `subagent_type: architect-reviewer`

```
Agent tool:
  subagent_type: architect-reviewer
  description: "Architect review: <scope>: <subject>"
  prompt: |
    You are the mandatory architectural reviewer for PR #<N> on savvagent/light (<ref>).

    Review the actual diff and the spec/plan for this change:
      gh pr diff <N>
      gh pr view <N> --json commits,files,title,body
      # Spec/plan (committed repo files): docs/superpowers/specs/<slug>-design.md,
      # docs/superpowers/plans/<slug>.md

    Check against light-factory's architecture (ARCHITECTURE.md "Execution model" + "Crate layout" —
    the intended destination — and the latest plan for what is actually shipped):
    - Inward dependency flow preserved: protocol (serde only) → auth → persistence → server → tui;
      auth must never depend on a concrete persistence impl; persistence must never depend on server
    - Crate boundaries respected; new capabilities added via the Store seam or new seams,
      never by widening public surface
    - The change matches the spec/plan it claims to implement; wire types stay semver-minor
      (additive only — a breaking change bumps the affected crate version(s) in Cargo.toml within
      this same PR, per Non-Negotiable Rule 6)
    - The human-in-the-loop architecture is preserved: any edit-application path is gated and
      fail-closed; nothing silently bypasses the human approval checkpoint
    - web/ stays a separate Svelte SPA; workspace build/test never requires node
    - The change follows the documented design rather than eroding it; deviations are justified

    Report: Strengths, Issues (Critical / Important / Minor), Assessment (Approve / Request changes).
```

### Independent security review — `subagent_type: security-auditor`

> **CRITICAL — this review is blind by design.** The security agent receives **only the diff**. It
> is never given the spec, the plan, the task brief, the PR body summary, the issue, or the
> implementer's report. Its findings must come from the code alone. This independence is a
> Non-Negotiable Rule — do not paste spec/plan context into this dispatch, and do not ask the agent
> to read the docs.

```
Agent tool:
  subagent_type: security-auditor
  description: "Independent security review: <scope>: <subject>"
  prompt: |
    You are the independent security reviewer for PR #<N> on savvagent/light.

    You receive ONLY the diff — deliberately. Do not read the PR description, the linked issue,
    any design spec or plan, or any implementer summary. Your findings must be derived from the
    code changes alone.

    The diff:
      gh pr diff <N>

    Evaluate the change for security defects, focusing hardest on light-factory's auth spine:
    - Account enumeration: any branch that distinguishes "unknown email" from "wrong TOTP code"
      (both must surface as invalid_credentials); register timing/oracle leaks
    - Secrets at rest: TOTP seeds and other secrets must be AES-256-GCM encrypted before the DB,
      under a key from LIGHT_SECRET_KEY that fails closed at startup; no plaintext secrets in the DB
    - Token handling: bearer tokens returned in full but stored only as SHA-256 hashes; never log
      or persist the raw token; session revocation must not require the raw token
    - Single-use semantics: registration challenges and RFC 8628 device grants must be consumed
      atomically (delete-and-return) — no replay, no double-spend, no race on concurrent polling
    - Device authorization: device_code high-entropy stored hashed only; user_code short-lived and
      single-use; no user_code enumeration or brute-force amplification
    - Secrets/PII: logging, exposing, or persisting secrets, TOTP codes, device codes, or
      LIGHT_SECRET_KEY; sensitive data crossing a trust boundary
    - WS auth: the /ws endpoint authenticates via Bearer header or ?token= query param — check for
      token leakage into logs/referrers, and that unauthenticated upgrades are rejected
    - Injection: SQL (sqlx), path traversal, template injection, untrusted-input panics or overflows,
      CORS misconfiguration (must stay restricted to configured origins)
    - Cryptographic misuse: nonce reuse, weak randomness, algorithm confusion, key handling

    Report: Strengths, Issues (Critical / Important / Minor), Assessment (Approve / Request changes).
```

Aggregate all three reports — plus the pr-review-toolkit and Copilot findings — into the single PR
comment grouped by **Critical / Important / Suggestions / Strengths**. Critical/Important findings
from any reviewer must be fixed or explicitly dismissed (with reasoning) before merge.

---

## Review-Response Subagent — Phase 4 Step 9 — `subagent_type: general-purpose`

```
Agent tool:
  subagent_type: general-purpose
  description: "Address PR review feedback"
  prompt: |
    You are addressing PR review feedback on PR #<N> for <ref>.
    Follow light-factory-development requirements (Phase 4 step 9).

    Your job:
    - Read all unresolved review threads on the PR (including the aggregated rust-pro,
      architect-reviewer, and independent security-auditor findings if they were posted as comments)
    - For each comment: either fix-and-reply ("Fixed in <sha>") or explicitly dismiss
      with reasoning. NEVER silent dismissal.
    - After each reply, resolve the conversation thread via GraphQL:
        gh api graphql -f query='mutation {
          resolveReviewThread(input: {threadId: "<thread_id>"}) {
            thread { isResolved }
          }
        }'
    - Always reply inline to each comment explaining how the feedback was addressed
      (keeps the review thread traceable).
    - For automated reviewers, a flagged false positive should be verified then dismissed
      with reasoning.
    - Do all fix work in the existing worktree (.worktrees/<branch>) and push to the PR
      branch — never commit to master directly.
    - Never add AI attribution to commit messages or replies.
    - If the same thread remains unresolved across multiple subagent runs, escalate
      (do not silently retry).

    Return when all threads are resolved or escalation is needed. The main thread
    receives only the summary (what was fixed, what was dismissed, any escalations).
```
