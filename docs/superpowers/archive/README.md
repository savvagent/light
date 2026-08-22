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
| `engine-core` | The engine thin vertical slice: Command/Event vocabulary, sensitive-path floor, `engine-core`/`providers`/`tools`/`engine` crates, `PlanGate`, session actor, turn state machine, and TUI wiring |
| `port-llm-providers` | Ported otto's seven providers, the `base_url` trust boundary, and env-driven selection with a TUI `/ask` command |
| `offline-provider-selection-reason` | Surfaces *why* provider selection fell back to the offline `LocalProvider` (`OfflineReason` + warnings), reports `no_provider_configured` instead of `invalid_plan`, and shows the reason in the TUI engine pane |
| `provider-credentials-ui` | In-client provider/model/key commands (`/provider`, `/model`, `/key`) with OS-keyring credential storage, masked input, redaction, and injectable `Selection`/`SelectedBy` selection |
| `engine-core-loose-ends` | Resolved the five engine-core loose ends from issue #21: removed dead step events (semver-major `0.1.1` → `0.2.0`), dropped the write-only `Session::approved`, tore engine mode down on leave, rejected mid-turn prompts visibly, and errored on non-UTF-8 `fs.read` |
| `bash-stdin-timeout` | Bounded `bash` tool calls — null stdin, wall-clock timeout killing the whole process group, kill-on-drop (plan only; fast-path bug fix) |
| `engine-prompt-ad-keys` | TUI engine prompt `a`/`d` approve/deny only while a decision is pending, typed as text otherwise (plan only) |
| `gate-bash-sensitive-floor` | `PlanGate` now floor-checks every command argument, so an `ArgPattern::Any` scope cannot read `.env` or overwrite `.git/config` (plan only) |
| `tui-broadcast-lag` | Engine-event forwarder continues on `RecvError::Lagged` and surfaces a "dropped events" notice instead of terminating (plan only) |
| `turn-step-budget-transcript` | Bounded the execute loop with `MAX_STEPS_PER_TURN` and truncated transcript entries with `MAX_TRANSCRIPT_ENTRY_CHARS` (plan only) |
| `help-modal` | Moved the TUI's per-mode help hints into a Ctrl-P help modal and replaced the status line with a short "Ctrl-P: help" indicator (plan only; fast-path) |
| `connect-modal` | Guided `/connect` modal (provider → API key → model) replacing `/provider`, plus a `providers::models` module that lists provider model ids behind the base-URL trust boundary |
| `models-command` | `/models` modal listing the active provider's models with the current one pre-highlighted, plus a scrolling popup viewport, a config-path seam making the apply path testable, and rollback on a failed settings save |
| `resolve-key-test-env` | Made the `resolve_key` keyring test independent of the ambient environment via an injected env reader, completing the seam for `key_status` too; the old test printed a real exported `OPENAI_API_KEY` verbatim into failure output |
| `models-fetch-bounds` | Bounded the model-list fetch: a per-request deadline funnelled through one `model_list_request` chokepoint, a streaming byte cap, refusal (not truncation) of an over-long list, `JoinHandle` abort on close, and a panic guard so a panicked fetch reports instead of spinning forever |
| `models-fetch-error-classes` | Branched the `/models` modal on *why* the fetch failed — credential failures point at `/connect`/`/key`/`/model <id>`, transport failures offer Ctrl+R retry plus an explicitly-unverified manual entry — and sized the popup from wrapped rows, which closed #57 |
| `tui-modal-module` | Extracted the help/connect/models modal machinery into `crates/tui/src/modal.rs` behind one `modal: Option<Modal>` host, making "only one modal open" unrepresentable and giving fetch invalidation a single seam |
