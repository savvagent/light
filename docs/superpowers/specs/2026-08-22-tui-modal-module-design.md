# TUI modal module — design

> **Status:** DRAFT — extract the help/connect/models modal machinery out of `app.rs` into
> `crates/tui/src/modal.rs`, behind a single `modal: ModalHost` field that makes "two modals open at
> once" unrepresentable.

> **Implements:** https://github.com/savvagent/light/issues/46

## 1. Brief

Issue #46, raised by the architectural review of #43:

> `crates/tui/src/app.rs` is now ~3400 lines and `App` carries ~40 fields. There are three modals
> (help, connect, models) that have converged on a recognisable shape — state enum, pure transition
> fn, enter/close/apply, `UiEvent` variant, nonce guard, `draw_*` over `draw_popup`. That
> convergence is good; the problem is the pattern is instantiated three times by copy, and two of
> the shared pieces (`draw_popup`, `fetch_model_list`) had to be retrofitted during #43 to stop
> drift.
>
> Two structural smells the third instance exposed:
>
> - **Modality is represented two ways.** Help and key entry are `Mode` variants; connect and models
>   are orthogonal `Option<…>` fields, checked in a hand-ordered cascade in `handle_key`.
> - **"Only one modal is open at a time" is unenforced.** `enter_models` does not check
>   `self.connect.is_none()`, and `draw` renders both blocks sequentially. Currently unreachable by
>   control-flow ordering, but not by construction. A single `modal: Option<Modal>` enum would make
>   the bad state unrepresentable and collapse six `App` fields into one.
>
> Worth doing **before a fourth modal is added**, while the move is still mechanical. Note this is
> scope *reduction*: six fields become one, three near-identical draw tails become one seam.
>
> Carry forward the `Close`/`Apply`/`Step` transition shape from `/models` — it is the better of the
> two designs, since `/connect` re-inspects the step after the fact to decide whether to apply.
>
> Also fold in: `models_return`/`connect_return` are inert (the value written always equals the
> value already held, because neither modal changes `mode`), and both should be deleted or
> re-derived from `self.session` on close.

## 2. Premise corrections

The issue's premises largely survive contact with the code. Three need correcting before the design
can be built to them:

1. **The three modals have *not* all converged on the same shape.** `/connect` and `/models` have
   (state enum + pure transition fn + `UiEvent` variant + nonce guard + `draw_popup`). Help has
   none of those: it is a `Mode` variant, has no state, no transition function, no async fetch, no
   nonce, and does **not** render through `draw_popup` — `draw_help` renders a
   `centered_rect(80, 90, …)` paragraph *instead of* the base screen (`app.rs:1438-1470`,
   `app.rs:1776`). Help is a modal in the "an overlay owns the keyboard" sense only. The design
   below therefore unifies **ownership and routing** across all three, and unifies the **popup draw
   tail** across the two that share it, rather than pretending help renders like the other two.

2. **The six fields collapse to two, not one — unless the nonce is wrapped.** The six are `connect`,
   `connect_return`, `connect_nonce`, `models`, `models_return`, `models_nonce`. `*_return` are
   inert and get deleted (§4.5). The nonce cannot live *inside* `Option<Modal>`: it is bumped
   **on close** precisely so a fetch that outlives the modal is discarded (`close_models`,
   `dismiss_modals`), so it must outlive the `Option`. Rather than leaving a bare
   `modal_nonce: u64` beside `modal: Option<Modal>` — reintroducing exactly the "remember to bump
   the other field" coupling the issue objects to — the design wraps both in one
   `ModalHost { open: Option<Modal>, nonce: u64 }`. `App` gains **one** field, `modal: ModalHost`.
   With `help_return` also deleted, **seven** `App` fields become one.

3. **`Mode::Key` (key entry) is out of scope.** The issue names it as *evidence* of the
   "modality is represented two ways" smell, but the change it asks for is the three modals and the
   six fields. Key entry is a full-screen form with its own three fields (`key_target`, `key_input`,
   `key_return`, where `key_return` is **not** inert — `begin_key_entry` really does change `mode`),
   reached from a command rather than layered over a screen. Folding it in is a separate, larger
   change; a follow-up issue is filed instead (§9).

## 3. Scope

**In:**

- New module `crates/tui/src/modal.rs` holding the modal state types, their pure transition
  functions, the popup renderer, and the model-list fetch.
- `App` gains a single `modal: ModalHost` field; `connect`, `connect_return`, `connect_nonce`,
  `models`, `models_return`, `models_nonce`, `help_return` and `Mode::Help` are deleted.
- One key-routing seam, one draw seam, one fetch-start seam, one close seam.
- `ConnectTransition` is replaced by the `Close`/`Apply`/`Step` shape carried forward from
  `/models`, so `/connect` no longer re-inspects its step after the fact to decide whether to apply.
- Every existing modal test moves with the code it covers and keeps passing.

**Out:**

- Any change to modal *behaviour*: same keys, same strings, same transitions, same rendered output.
  The two exceptions the issue sanctions are named in §7.
- `Mode::Key` / key entry (§2.3) — follow-up issue.
- Any change to `crates/providers`, the fetch timeouts/bounds (#44), or the fetch error
  classification (#47). The design names where those land (§8) but does not implement them.
- Any new i18n key. No string moves catalogs; every `t()`/`t_with()` key is carried verbatim, so
  `en`/`es` parity is untouched by construction.
- Any public interface, wire type, crate boundary, or dependency-flow change. `UiEvent` is `pub` in
  a private module of the **binary** crate `light-factory`, reachable only from `main.rs` via
  `app::run`; it is not a library API and carries no semver obligation.

## 4. Design

### 4.1 Module layout

`crates/tui/src/modal.rs`, declared `mod modal;` in `crates/tui/src/main.rs` (the binary crate —
`app.rs` lives there too; `crates/tui/src/lib.rs` stays as-is). Everything in it is `pub(crate)`.

| Item | Moved from | Why it belongs here |
|---|---|---|
| `ProviderRow` | `app.rs:86` | Connect-modal row |
| `ConnectStep` + its hand-written redacting `Debug` | `app.rs:94`, `app.rs:115` | Connect-modal state |
| `ModelsStep` | `app.rs:165` | Models-modal state |
| `Modal`, `ModalHost`, `ModalTransition`, `ModalApply`, `PopupView`, `ModalContext` | new | The unified seam |
| `connect_step_next` | `app.rs:2219` | Pure transition |
| `models_step_next` | `app.rs:2387` | Pure transition |
| `models_apply_target` | `app.rs:2331` | Pure apply target |
| `help_lines` | `app.rs:2489` | Help-modal body |
| `fetch_model_list` | `app.rs:2358` | Shared by both fetching modals |
| `draw_popup`, `centered_rect` | `app.rs:2549`, `app.rs:2610` | The shared popup renderer |
| `cycle_index` | `app.rs:2202` | Used only by the two transition fns |

`ConnectTransition` / `ModelsTransition` are **deleted**, replaced by one `ModalTransition` (§4.3).

Two helpers stay in `app.rs` and become `pub(crate)` so `modal.rs` can reach them:

- `mask` (`app.rs:2197`) — also used by `draw_key`, which is not a modal.
- `takes_key` (`app.rs:2165`) — also used by `begin_key_entry` and `clear_key`, and read by
  `connect_step_next`'s model-list Esc branch (`app.rs:2296`) to decide whether Esc routes back to
  key entry or to the provider list.

`PROVIDER_NAMES` / `is_valid_provider` / `build_provider_rows` stay in `app.rs` unchanged:
building the rows needs `self.store`, so it was never part of the pure machinery.

### 4.2 The state

```rust
/// The single overlay that owns the keyboard, if any. One variant per modal makes
/// "two modals open at once" unrepresentable.
pub(crate) enum Modal {
    Help,
    Connect(ConnectStep),
    Models(ModelsStep),
}

/// Owns the open modal together with the counter that invalidates its in-flight fetch.
/// The nonce outlives the modal on purpose: closing bumps it so a fetch already in flight
/// is discarded when it lands.
pub(crate) struct ModalHost {
    open: Option<Modal>,
    nonce: u64,
}
```

`ModalHost`'s API is the whole point — it is the only way to mutate modal state, so the
"closing bumps the nonce" invariant cannot be forgotten at a call site:

| Method | Contract |
|---|---|
| `open(&mut self, modal: Modal)` | Replace whatever is open. Bumps the nonce (the outgoing modal's fetch, if any, is invalidated). |
| `close(&mut self)` | Clear and bump the nonce. No-op nonce bump when nothing was open, matching today's `dismiss_modals` guard. |
| `replace_step(&mut self, modal: Modal)` | Swap the state of the *same* modal without bumping — a step transition must not invalidate its own in-flight fetch. |
| `current(&self) -> Option<&Modal>` / `current_mut` | Read/patch the open modal (used by the fetch-result handler). |
| `is_open(&self) -> bool` | For `handle_key`'s Ctrl-P guard and `draw`. |
| `nonce(&self) -> u64` | Stale-result guard. |
| `next_fetch_nonce(&mut self) -> u64` | Bump and return, for a newly-spawned fetch. |

`ModalHost::default()` is `{ open: None, nonce: 0 }`, matching today's initial state.

`Modal::Help` carries no payload. It does not touch `self.mode`, so `help_return` becomes inert and
is deleted; the existing tests that assert "closing help returns to the mode it was opened from"
still hold, because the mode is now never left in the first place.

### 4.3 The transition shape

Carried forward from `/models`, applied to all three:

```rust
pub(crate) enum ModalTransition {
    /// Stay open in a new state.
    Step(Modal),
    /// Commit the modal's selection, then close.
    Apply,
    /// Close, committing nothing.
    Close,
}

impl Modal {
    /// Pure: maps a key press in the current state to the next state, apply, or close.
    /// No keyring, terminal, or network state.
    pub(crate) fn next(&self, key: KeyEvent) -> ModalTransition;

    /// What `Apply` should commit, or `None` when the state carries no usable selection.
    pub(crate) fn apply_target(&self) -> Option<ModalApply>;
}

pub(crate) enum ModalApply {
    /// `/connect`: adopt `provider` as the preferred provider and pin `model` to it.
    Provider { provider: String, model: String },
    /// `/models`: pin `model` to the already-active `provider`.
    Model { provider: String, model: String },
}
```

`Modal::next` dispatches to `help_step_next` / `connect_step_next` / `models_step_next`. The two
existing transition functions keep their exact key mappings; only their return type changes:

- `ConnectTransition::Step(s)` → `ModalTransition::Step(Modal::Connect(s))`.
- `ConnectTransition::Close` **splits**. Today `handle_connect_key` maps it to
  `apply_and_close_connect`, which re-inspects the step to decide whether anything is applied
  (`app.rs:783`). The two producers become explicit:
  - `ProviderList` + Esc → `Close` (today: `Close` → `apply_and_close_connect` finds no `ModelList`
    step → applies nothing).
  - `ModelList` + Enter, not fetching, non-empty → `Apply` (today: `Close` → applies the highlighted
    model).

  Observably identical; the after-the-fact re-inspection is gone.
- `ModelsTransition` maps one-for-one (`Step`/`Apply`/`Close`).
- Help: `Esc` or `Ctrl-P` → `Close`; anything else → `Step(Modal::Help)`. Ctrl-C is intercepted
  before `next` is reached (below), exactly as today.

`apply_target` is `models_apply_target` generalised: `Modal::Connect(ModelList { .. })` yields
`ModalApply::Provider`, `Modal::Models(..)` delegates to the existing `models_apply_target` logic
and yields `ModalApply::Model`, `Modal::Help` yields `None`.

### 4.4 The four seams in `App`

**Key routing.** `handle_key`'s hand-ordered cascade (`app.rs:313-338`) — `mode == Help`, then the
Ctrl-P guard's `!command_mode && connect.is_none() && models.is_none()`, then `command_mode`, then
`Engine`, then `Key`, then `connect.is_some()`, then `models.is_some()` — becomes:

```rust
if self.modal.is_open() {
    return self.handle_modal_key(key);
}
if ctrl_p && !self.command_mode { self.open_help(); return false; }
… command_mode / Engine / Key / base, unchanged …
```

The `connect.is_none() && models.is_none()` clauses in the Ctrl-P guard are now dead by
construction and are deleted with it. Equivalence: the only key a modal ever declined to handle was
Ctrl-C (each of the three handlers returns `true` for it), and `handle_modal_key` keeps that check
first.

```rust
fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
    if ctrl_c(key) { return true; }
    // `Modal` derives `Clone`; cloning releases the borrow on `self.modal` so the arms below can
    // take `&mut self`. This is what `handle_connect_key`/`handle_models_key` already do today.
    let Some(current) = self.modal.current().cloned() else { return false };

    // Connect-specific pre-steps. Both need `&mut self` (status/error/keyring) and so cannot live
    // in the pure transition; both are lifted verbatim from `handle_connect_key`.
    //   1. Blank Enter on `KeyEntry` short-circuits to `status.key_empty` without stepping.
    //   2. `KeyEntry → ModelList { from_key: true }` writes the typed key to the keyring first,
    //      and on failure sets `self.error` and returns without stepping or fetching.
    // Step 2 yields the `key_override` handed to `begin_model_fetch` below.

    let before = current.fetch_target().map(str::to_string);
    match current.next(key) {
        ModalTransition::Close => self.modal.close(),
        ModalTransition::Apply => self.apply_and_close_modal(),
        ModalTransition::Step(next) => {
            let after = next.fetch_target().map(str::to_string);
            self.modal.replace_step(next);
            if let Some(provider) = after.filter(|a| Some(a) != before.as_ref()) {
                self.begin_model_fetch(provider, key_override);
            }
        }
    }
    false
}
```

`ModalTransition::Step` carries a whole `Modal`, so it could in principle name a different modal
kind; no transition function does, and `replace_step` deliberately does **not** bump the nonce
(a step must not invalidate its own in-flight fetch). What protects a step-to-step transition from
a stale result is the provider/step-shape match inside `handle_models_fetched`, exactly as today:
stepping `ModelList{fetching} → ProviderList` on Esc leaves the fetch running, and the result is
dropped because the open step is no longer a matching fetching list
(`connect_model_list_enter_is_a_noop_while_fetching`, `models_fetch_result_does_not_clobber_manual_entry`).

**Close.** `close_connect` (`app.rs:777`), `close_models` (`app.rs:965`), `close_help`
(`app.rs:422`) and `dismiss_modals` (`app.rs:633`) all collapse to `ModalHost::close()` — one close
path where there were four. The
`self.mode = self.*_return` lines go away with the fields (§4.5). `dismiss_modals` keeps its name
and its two call sites (`sign_out`, the `ws_closed` branch of `handle_server`) and becomes a
one-line delegation — the doc comment on it stays, because *why* the session teardown dismisses the
modal is not obvious from the call.

**Apply.** `apply_and_close_connect` + `apply_and_close_models` collapse to:

```rust
fn apply_and_close_modal(&mut self) {
    let apply = self.modal.current().and_then(Modal::apply_target);
    self.modal.close();
    match apply {
        Some(ModalApply::Provider { provider, model }) => {
            let previous = self.settings.provider.replace(provider.clone());
            if !self.persist_model(provider, model) { self.settings.provider = previous; }
        }
        Some(ModalApply::Model { provider, model }) => { self.persist_model(provider, model); }
        None => {}
    }
}
```

Ordering note: `apply_and_close_models` closes *before* persisting; `apply_and_close_connect`
persists *before* closing. Neither ordering is observable — `persist_model` touches `settings`,
`provider_info`, `status`/`error`, never `connect`/`models` — and closing first is the models
shape the issue asks to carry forward. The unified function closes first.

**Draw.** `draw`'s two sequential `if …is_some()` blocks (`app.rs:1467-1473`) become one:

```rust
// Help replaces the screen underneath it; connect and models float over it.
if !self.modal.covers_base() {
    match self.mode { … the seven screen arms … }
}
self.draw_modal(frame, chunks[1]);
```

Deleting the `Mode::Help` variant touches three further sites, each of which must be carried over
rather than dropped:

- `draw`'s screen match (`app.rs:1455-1465`) loses its `Mode::Help` arm — seven arms remain.
- `draw`'s footer hint (`app.rs:1477`) branches `self.mode == Mode::Help` → `hint.help_close`. It
  becomes `matches!(self.modal.current(), Some(Modal::Help))`, keeping the same key and the same
  precedence (after `command_mode`, before `Mode::Device`).
- `handle_key`'s Esc match (`app.rs:365`) loses its no-op `Mode::Help => {}` arm.

`Modal::covers_base()` is `matches!(self, Modal::Help)` and `ModalHost::covers_base()` is `false`
when nothing is open. This is the honest statement of premise correction §2.1: help is opaque,
the other two are overlays. It preserves today's rendering exactly — today `Mode::Help` in the
screen match means the base screen is never drawn under help.

`draw_connect` and `draw_models` lose their duplicated tail (footer styling + focus computation +
`draw_popup` call) to one seam:

```rust
fn draw_modal(&self, frame: &mut Frame, area: Rect) {
    let Some(modal) = self.modal.current() else { return };
    let ctx = ModalContext { locale: self.config.lang, error: self.error.as_deref(),
                             offline: self.provider_info.offline.as_ref() };
    match modal.view(&ctx) {
        ModalView::Popup(view) => draw_popup(frame, area, view),
        ModalView::FullScreen(view) => draw_full_screen(frame, area, view),
    }
}
```

`PopupView { title: String, body: Vec<Line<'static>>, footer: String, focus: Option<usize> }` is
owned throughout (`footer` is a `String` where `draw_models` used a `&str`, one trivial allocation
per frame) so the view outlives the borrow of `App` that built it; every line today is already
either an owned `String`/`format!` or a `&'static str` from `i18n::t`, so `Line<'static>` costs
nothing. It is
built by `Modal::view`, which is the moved bodies of `draw_connect`/`draw_models` minus their
`draw_popup` call, plus a `Help` arm returning `ModalView::FullScreen`. The help arm renders through
the same `centered_rect(80, 90, …)` + bordered `Paragraph` + `Wrap { trim: false }` it uses today —
byte-identical output, not `draw_popup`.

`ModalContext` carries the three pieces of `App` the views read today: the locale, `self.error`
(rendered inside the connect key-entry and model-list steps), and `provider_info.offline` (rendered
by the models offline step via `crate::provider::offline_notice`).

**Open.** `enter_connect`, `enter_models` and `open_help` all route through one
`App::open_modal(&mut self, modal: Modal, key_override: Option<String>)`, which calls
`ModalHost::open` and then applies the fetch rule of §4.6 (`before` is the outgoing modal's fetch
target, `after` the incoming one's). This is why `enter_models`'s fetch does not need its own
`begin_models_fetch` call: opening a modal whose state is a fetching list starts the fetch through
the same seam a step transition does. `enter_connect` opens a `ProviderList` (no fetch target, no
fetch), `open_help` opens `Modal::Help` (likewise).

**Fetch.** `begin_fetch` (`app.rs:805`, connect) and `begin_models_fetch` (`app.rs:884`, models) are
byte-for-byte identical apart from the key override and the `UiEvent` variant they send. They
collapse to one `begin_model_fetch(&mut self, provider: String, key: Option<String>)`, which claims
its nonce from `ModalHost` and posts the event the open modal expects.

**The two `UiEvent` variants are deliberately *not* merged.** An earlier draft of this spec merged
`UiEvent::ConnectModels` into `UiEvent::ModelsFetched` on the grounds that their payloads were
identical. Sibling PR #55 (issue #47, which merges before this branch) makes them genuinely
different: `ModelsFetched` gains a richer payload (`FetchFailure`/`FetchError`/`ModelChoice`) so the
models modal can classify an auth failure apart from a network failure, while `ConnectModels`
deliberately keeps its `Result<Vec<String>, String>` so the connect modal is untouched. Two
differently-typed results are not duplication, and merging them here would have to be un-merged on
the rebase. `handle_connect_models` and `handle_models_fetched` therefore both survive, each keeping
its own fill — connect shows a fetch error **inline** in its model-list step, models falls back to a
**manual entry** step — and both are now guarded by the one `ModalHost` nonce instead of two
per-modal counters. The unification this change does deliver is the nonce, the launcher, and the
close path.

### 4.5 The inert `*_return` fields

`connect_return`, `models_return` and (once help stops changing `mode`) `help_return` are deleted
outright rather than re-derived. Justification, per the issue's "delete them or re-derive from
`self.session` on close":

- `connect_return`/`models_return` are written in `enter_connect`/`enter_models` as
  `self.<x>_return = self.mode`, and read back in `close_*` as `self.mode = self.<x>_return`.
  Neither modal ever assigns `self.mode` in between (verified: no `self.mode =` in
  `enter_connect`, `close_connect`, `apply_and_close_connect`, `handle_connect_key`,
  `handle_connect_models`, `begin_fetch`, or their models twins), so the read always restores the
  value already held. The write is a no-op and the read is a no-op.
- With `Modal::Help` no longer a `Mode`, the same becomes true of `help_return`.
- Re-deriving from `self.session` would be *worse*: it would newly couple modal close to session
  state and would actually change behaviour in the `/connect`-while-signed-out case that
  `dismiss_modals` exists to prevent. Deleting is the change that provably preserves behaviour.

The existing tests are the regression net for this and are kept (they assert on `app.mode`, which
is now simply never disturbed). They are renamed in the review round to
`closing_the_modal_leaves_the_base_mode_undisturbed` and
`help_modal_closes_without_disturbing_the_base_mode`, each carrying its old name in a doc line, so
the names stop promising a restore that no longer happens without losing the grep path to master.

**The distinction in the first two bullets is load-bearing and must not be flattened.**
`connect_return`/`models_return` were inert *on master*. `help_return` was **not**: master's
`open_help` is `self.help_return = self.mode; self.mode = Mode::Help;` — the assignment is the very
next line, and `close_help`'s read was the only way back out of `Mode::Help`. `help_return` became
inert *because* this change deletes the `Mode::Help` variant. Saying all three "were inert" reads
as a claim about master and is false of the third; it is also the claim that masked the regression
recorded in §11.1, and it is repeated in the body of follow-up issue #64, where the deferral of
`Mode::Key` rests on it. `key_return` is load-bearing on master in *precisely* the same way
`help_return` was — the reason #64 is deferred is that folding it in needs a `return_to` payload,
not that help was different in kind.

### 4.6 Starting a fetch on a step

Today, connect starts a fetch only on the *transition into* a fetching `ModelList`
(`handle_connect_key`, `app.rs:1013-1021`), and models starts one in `enter_models`. A naive
"fetch whenever the current step is fetching" would re-issue a request on every keypress while the
spinner is up. The seam therefore compares the fetch target across the transition:

```rust
/// The provider whose model list this state is waiting on, or `None`.
fn fetch_target(&self) -> Option<&str>;
```

A fetch is started iff `after != before` and `after` is `Some`. Walked through every reachable
transition: `ProviderList → ModelList{fetching}` (None → Some, fetch), `KeyEntry → ModelList{fetching}`
(None → Some, fetch), `ModelList{fetching} → ModelList{fetching}` (Some(X) → Some(X), no fetch),
`ModelList{fetching} → ProviderList` on Esc (Some → None, no fetch), and `open()` from closed
(None → target, so `enter_models`'s fetch starts through the same seam). This reproduces today's
call sites exactly.

The just-typed API key that connect passes as `key_override` stays where it is: the
`KeyEntry → ModelList{from_key: true}` pre-step in `App` that writes the key to the keyring (it
needs `self.store`, so it cannot move into the pure transition) hands the trimmed key to
`begin_model_fetch`. `fetch_model_list` is unchanged and still consumes the key without returning it.

## 5. Goal & Success Criteria

Goal: one representation of "a modal is open", one place that opens/steps/closes/applies one, and
one module holding the machinery — so the fourth modal is a variant plus a transition function, not
a fourth copy of the pattern.

- [ ] `App` holds exactly one modal field (`modal: ModalHost`); `connect`, `connect_return`,
      `connect_nonce`, `models`, `models_return`, `models_nonce`, `help_return` and `Mode::Help` no
      longer exist. `App` field count drops by 6: **44 → 38** (44 counted at `app.rs:192-238`; the
      issue's "~40" is approximate — seven fields are removed and one added).
- [ ] Two modals open simultaneously is not representable: `ModalHost::open` is a single
      `Option<Modal>`, and `open()` replaces.
- [ ] `crates/tui/src/app.rs` sheds at least 800 lines (from 3736); `crates/tui/src/modal.rs` holds
      the modal types, transitions, views, popup renderer and fetch. The absolute line count is
      reported, not targeted.
- [ ] All 63 tests currently in `app.rs`'s `#[cfg(test)] mod tests` still exist and pass — those
      covering moved code move with it into `modal.rs`'s own test module. Renaming is allowed,
      deletion is not; the plan's final task re-counts them. `cargo test --workspace` is green.
- [ ] `cargo clippy --workspace --all-targets -D warnings` and `cargo fmt --all --check` are clean.
- [ ] Rendered output is unchanged: the help/connect/models render tests pass byte-for-byte with no
      expectation edits.

## 6. Error Handling & Edge Cases

| Case | Behaviour (unchanged) |
|---|---|
| Fetch result lands after the modal closed | `ModalHost::close` bumped the nonce; the result's nonce no longer matches and it is dropped. |
| Fetch result lands after a *different* modal opened | `open()` bumps the nonce too, so the stale result is dropped by the nonce guard before the modal-kind match is reached. Today this was guarded by two separate counters plus a step-shape check; the merged counter is strictly stronger. |
| Fetch result for a provider the step is no longer waiting on | The provider/step-shape match after the nonce check, kept verbatim from both handlers (`models_fetch_result_is_ignored_when_the_provider_does_not_match`, `models_fetch_result_does_not_clobber_manual_entry`). |
| Session lost with a modal open | `dismiss_modals` → `ModalHost::close`: modal cleared, nonce bumped, in-flight fetch invalidated. `Modal::Help` is now dismissed too — previously the `self.mode = Mode::SignIn` assignment that precedes the call clobbered `Mode::Help`, which is the same visible result. |
| Ctrl-C with a modal open | Quits. Checked first in `handle_modal_key`, before `Modal::next`. |
| Ctrl-P with the connect key-entry step focused | Still appends `p` to the key input — `connect_step_next`'s `KeyCode::Char(c)` arm is unchanged and the Ctrl-P guard is now unreachable while a modal is open (as it already was). |
| Blank Enter on the connect key-entry step | Still short-circuits to `status.key_empty` in `App` before the transition runs; that pre-step needs `self.status` and stays in `App`. |
| Keyring write fails on the connect key step | Still sets `self.error`, returns without stepping, and starts no fetch. |
| Tiny terminal | `draw_popup` unchanged, including its `u16::try_from` clamp for a remote-supplied list long enough to overflow `u16` (`a_popup_on_a_tiny_terminal_does_not_panic`). |
| `ConnectStep::KeyEntry` in a `Debug` rendering | The hand-written redacting `Debug` impl moves with the type, unchanged (`connect_step_debug_redacts_the_key`). |

## 7. Behaviour deltas (both sanctioned by the issue)

1. **The single-modal invariant is now enforced by construction.** `enter_models` did not check
   `self.connect.is_none()` and `draw` rendered both blocks sequentially. With one
   `Option<Modal>`, opening a modal replaces any other. Unreachable today by control-flow ordering,
   so no user-visible change; the point is that it is now unreachable by construction.
2. **`connect_return` / `models_return` / `help_return` are deleted.** Inert at the point of
   deletion (§4.5); deleting them cannot change behaviour. Note the asymmetry recorded there:
   `help_return` was load-bearing on master and is made inert *by* this change.
3. **A keypress on a still-loading connect list no longer respawns its fetch.** Undeclared in the
   first draft of this spec and found independently by three reviewers. Master's
   `handle_connect_key` fired a fetch on *any* step landing on `ModelList { fetching: true, .. }`,
   and `connect_step_next`'s `Up`/`Down` arm carries `fetching` through — so every keypress while a
   connect list was loading spawned a duplicate request, bumped the nonce, and discarded the
   in-flight result. Each redundant request carried the provider API key, and terminal key-repeat
   triggers it. Worse, the re-fetch passed `fetch_key = None`, dropping the key the user had just
   typed in favour of `resolve_key`. The `after.filter(|a| Some(a) != before.as_ref())` guard in
   `handle_modal_key` fixes it; `moving_the_cursor_while_a_connect_list_loads_does_not_refetch`
   pins it. A genuine improvement, claimed here rather than left for a future bisection to blame
   this change for an unconfessed delta.

Everything else is required to be observably identical, and the moved tests are the check.

## 8. Absorbing the parallel work (#44, #47)

This branch merges after #45, #44 and #47. The design leaves each an obvious landing spot; neither
is implemented here.

- **#44 (`models-fetch-bounds`)** stores the fetch `JoinHandle` and aborts it on close. `ModalHost`
  is exactly that seam: a third private field `fetch: Option<JoinHandle<()>>`, set by
  `begin_model_fetch` (now **one** call site instead of two) and aborted in `invalidate_fetch` —
  see §11.2, which corrects an earlier statement of this landing spot that named only `open`/`close`
  and would have leaked the task on the step-back path. This is the same shape `leave_engine`
  already uses for `engine_forward_task` (`app.rs:483`). The abort belongs in `ModalHost` rather
  than in `App` precisely so it cannot be forgotten on one of the close paths — which is the bug
  class #44 exists to close. On a rebase conflict, #44's abort semantics land inside `ModalHost`.
- **#47 (`models-fetch-error-classes`)** branches `handle_models_fetched` on the error class and
  adds a `ModelsStep` variant, i18n strings and a draw arm. `ModelsStep` keeps its identity and
  moves whole to `modal.rs`; a new variant is a new arm in `models_step_next` and a new arm in the
  models `view`. `handle_models_fetched` keeps its name and its models-specific fill. On a rebase
  conflict, #47's classification logic lands in the models arm of the merged handler and its new
  step variant in `modal.rs`.

Merge order is #45 → #44 → #47 → this branch, so all three land first. Before the PR opens and
again before it merges, this branch rebases onto `origin/master` and re-runs the full suite.
**Issue #57 (`draw_popup` sizes from `body.len()`, not the wrapped line count).** Found by #47's
worker; a body string wider than the popup's ~58-column inner width wraps to two rows, the height
calculation counts it as one, and content is pushed off the bottom. **Not fixed here** — it is a
behaviour change and separately tracked. This change is neutral-to-helpful for it: after the draw
seam lands, `draw_popup` has exactly **one** call site and remains the only place the height is
computed, so #57 is a localized fix inside `draw_popup` rather than an edit spread over three draw
tails. The view builders deliberately do **not** compute or cap height; they only produce
`PopupView { title, body, footer, focus }`.

Conflict-resolution rule for the rebase: **their semantics win, this branch's structure wins** —
a behaviour change they introduced is preserved, expressed through the structure introduced here.
#45 is confined to `crates/tui/src/selection.rs` and cannot conflict.

## 9. Risks & Open Questions

1. **Risk: a silent rendering change.** Mitigation: the three render tests
   (`models_modal_renders_its_own_header`, `a_long_model_list_keeps_the_selection_and_footer_on_screen`,
   `an_empty_list_does_not_advertise_enter`, `the_offline_modal_names_the_actual_reason`) assert on
   a rendered terminal buffer and are kept with **no expectation edits**. Any drift fails them.
2. **Risk: the rebase onto #44/#47.** Mitigation: §8, plus the plan's task ordering keeps each
   commit independently green so a rebase can be replayed step by step.
3. **Risk: `Modal::view` needs more of `App` than `ModalContext` carries** as modals grow.
   Accepted: `ModalContext` is a `pub(crate)` struct in a binary crate; widening it is a one-line
   change. Today it needs exactly locale, `error`, and `provider_info.offline`.
4. **Open: key entry (`Mode::Key`) still represents modality the second way.** Deliberately out of
   scope (§2.3). A follow-up issue is filed on this branch's PR — *"tui: fold key entry into the
   modal host"* — describing the three fields (`key_target`, `key_input`, `key_return`), the
   full-screen render, and the fact that `key_return` is genuinely load-bearing (unlike the three
   `*_return` fields deleted here), so it needs a `Modal::Key { return_to: Mode }` payload or an
   equivalent. Filing, not fixing, is the correct move per the workflow's "do not silently widen
   scope" rule.
5. **Resolved in the review round: `covers_base` moved from `Modal` to `ModalView`.** It was stated
   per modal *and* checked a second time, as `matches!(.., Modal::Help)`, for the status hint. Both
   now come off the value that decides how the modal is painted: `ModalView::covers_base` is
   `matches!(self, ModalView::FullScreen(_))`, and `Modal::hint_key` folds in the second case. That
   removes the drift in which a modal claims to cover the base while rendering a `Popup`, which
   would paint over a screen nobody drew — `draw_full_screen` deliberately does not `Clear`. Still
   an exception rather than a uniform rule, but no longer one that can disagree with itself.
6. **Not a risk: semver.** No public interface changes; `light-factory-tui`'s library surface
   (`lib.rs`: `credentials`, `engine_view`, `i18n`) is untouched. No `Cargo.toml` version bump.

## 10. Assumptions

1. **The module is `crates/tui/src/modal.rs` in the binary crate, not the library.** Rationale:
   `app.rs` is a binary module and the modal machinery is only reachable from it; promoting it to
   `lib.rs` would create a public API surface (and a semver obligation) the issue does not ask for.
2. **Help joins `Modal` despite not sharing the popup shape.** Rationale: the issue's first smell is
   that modality has two representations; leaving help as a `Mode` variant would leave the smell
   half-fixed and the `handle_key` cascade still hand-ordered.
3. **Key entry does not join `Modal`.** Rationale: §2.3 — the issue scopes the change to three
   modals and six fields, and `key_return` is genuinely load-bearing.
4. **The two nonces merge into one.** Rationale: only one modal can be open, so one counter is
   sufficient; a merged counter also invalidates across a modal *switch*, which two counters did
   not. Strictly stronger, not observably different.
5. **The two `UiEvent` fetch variants stay separate.** Rationale: sibling #47 gives them different
   payloads (§4.4). They already share one launcher and one nonce; merging the variants themselves
   would fight a change that merges first, for no structural gain.
6. **`apply_and_close_modal` closes before persisting.** Rationale: that is the `/models` ordering
   the issue asks to carry forward, and the orderings are indistinguishable (§4.4).
7. **No i18n change.** Rationale: no string is added, removed, or re-keyed; keys move file but not
   catalog, so `en`/`es` parity is preserved by construction.
8. **Tests move to sit beside the code they cover.** Rationale: repo convention is tests next to
   code; the pure-transition tests follow `connect_step_next`/`models_step_next`/`help_lines` into
   `modal.rs`, while tests that drive an `App` (routing, apply, persistence, rendering) stay in
   `app.rs`. A move is not a deletion — the plan's final step re-counts them.

## 11. Review-round design changes

Seven reviews plus an architect review ran against the opened PR. Everything below changed the
design, not just the code; the rest of the spec above is unchanged and still describes the target.

### 11.1 A modal must be dismissed when the mode changes asynchronously

The first draft wired `dismiss_modals` to the two synchronous session-loss paths only, and its
comment claimed "a modal cannot float over the sign-in screen or swallow its keys." It could.
`handle_device_result` reaches `Mode::SignIn` on error and `enter` reaches `Mode::Connected` on
success, both asynchronously, and Ctrl-P is reachable from `Mode::Device` because `handle_key`'s
Ctrl-P branch runs before the mode dispatch. Left open, `covers_base` suppresses the replacement
screen entirely and `handle_modal_key` swallows every key but Ctrl-C.

This was impossible on master, where help *was* `Mode::Help`, so assigning `self.mode` tore the
overlay down. Decoupling help from `Mode` is what introduced it — which is exactly the claim §4.5
now insists must not be flattened. Both asynchronous arrivals dismiss.

### 11.2 The nonce bump *is* the fetch-invalidation event

The first draft had three mutators disagreeing about invalidation: `close` early-returned when
nothing was open, `replace_step` never bumped, `next_fetch_nonce` bumped directly. All three now
route through one private `ModalHost::invalidate_fetch`, and `replace_step` bumps exactly when
`Modal::fetch_target` changes across the step.

The rule this establishes, and the reason it matters more than tidiness: **abort wherever the nonce
is bumped.** #56's `JoinHandle` belongs in `ModalHost` with its `abort()` inside `invalidate_fetch`,
which reaches all four mutators. Naming only `open`/`close` — as §8 originally did — misses the
path `ProviderList → Enter → ModelList{fetching}` (task A spawned) `→ Esc → ProviderList` (no open,
no close) `→ Enter → ModelList{fetching}` (task B spawned, handle overwritten), where task A runs to
completion holding a connection whose headers carry the provider API key. #56 shipped a fix for
precisely that path (`stepping_back_out_of_a_fetching_connect_list_aborts_the_fetch`); it must keep
passing under this structure. `close` is unconditional for the same reason: a fetch outlives the
modal that started it, so modal state is not evidence about fetch state.

One consequence, recorded so nobody re-derives it: with `replace_step` bumping, every path to
`begin_model_fetch` is already preceded by a bump, so `next_fetch_nonce()` and `nonce()` return the
same value there. `next_fetch_nonce` is kept anyway, because it is also the abort seam — see §11.9,
which is what makes the distinction observable again.

### 11.3 `ModalTransition::Apply` carries what it commits

A nullary `Apply` stated the rule "this step has something to commit" twice — in the transition arm
and again in `Modal::apply_target` — and `apply_and_close_modal` re-read the modal to discover
which. That second read happened after `self.store.set(..)` and interleaved with
`self.modal.close()`, and it selected between commits of *different privilege*: `ModalApply::Provider`
changes which service receives the user's prompts and key, `Model` only re-pins a model on the
provider already active. The two statements could also disagree — `Modal::Help` returning `Apply`
compiled, and the `None => {}` arm then closed the modal having committed nothing.

`Apply(ModalApply)` makes it structural. Every arm producing one already had `provider`, `models`
and `selected` in scope, so the payload is free; `Modal::apply_target` and `models_apply_target` are
deleted, along with the duplicated enumeration of non-applying states that §4.3 had in both places.

### 11.4 `fetch_target` names the sink, and has no catch-all

`begin_model_fetch` chose its result event with `matches!(self.modal.current(), Some(Modal::Connect(_)))`
— the same after-the-fact re-inspection this change removed from `/connect`'s apply path, and
correct only by convention now that `replace_step` accepts any variant. `fetch_target` returns
`Option<(&str, FetchSink)>`, so the sink is decided by the arm that decided there was a fetch at
all. Its `_ => None` is gone: a fourth modal, or a fifth step, that should fetch would have fallen
through it and simply never started one.

### 11.5 The module points away from `app.rs`

`modal.rs` imported `mask` and `takes_key` back out of `app.rs`, a cycle that makes the module
unmovable — and a module that cannot be moved is not yet a layer. `takes_key` joins
`resolve_key`/`key_status`/`REMOTE_IDS` in `crate::selection`, which it already wraps; `mask` moves
into `modal.rs`, which is where key entry is heading under #64. The graph is now
`app → modal → {selection, provider, i18n, credentials}`.

### 11.6 #57 is fixed upstream, not here

`draw_popup` sized its box from `body.len()` and scrolled `focus` by the same logical count, while
ratatui counts wrapped rows. PR #55 fixed both (`4d98474`, closing #57) using
`Paragraph::line_count` behind ratatui's `unstable-rendered-line-info` feature, which
`crates/tui/Cargo.toml` now enables. This branch merged that and carried it across the module move;
the comments that described the unfixed behaviour are replaced by #55's, not reverted to
`body.len()`. `draw_popup` has exactly one call site, so the arithmetic has one home.

### 11.8 `ModelsTransition::Retry` is removed, not merged

#55 added a `Retry` transition and an `App::retry_models_fetch` that rebuilt a fetching
`ModelsStep::ModelList` and called `begin_models_fetch`. Under this branch's structure that is the
step itself: `handle_modal_key` starts a fetch whenever [`Modal::fetch_target`] goes `None -> Some`
across a transition, every step offering Ctrl+R has `None` as its target, and `replace_step`
invalidates the earlier fetch on the same target change that starts the new one. So Ctrl+R returns
a plain `Step` into a fetching list (`refetch`), and the transition variant, the `App` method and
`ModelsStep::provider` — whose only caller it was — all go.

The property the removal rests on is not obvious from the arms, so it is pinned:
`a_retry_step_begins_awaiting_a_fetch_the_step_before_it_was_not` asserts, for all three
retry-capable steps, that the step before names no fetch target and the step after names the
provider's.

### 11.9 The abort seam is the invalidation seam

#56 shipped after §11.2 was written and confirms it. Its `JoinHandle` lives in `ModalHost` and is
aborted inside `invalidate_fetch`, so the four mutators that bump the nonce are exactly the four
that cancel. That is what keeps #56's `stepping_back_out_of_a_fetching_connect_list_aborts_the_fetch`
passing here: Esc from a fetching connect list steps back through neither `open` nor `close`, and
`replace_step`'s target-change bump is the only thing that catches it.

It also settles §11.2's open note. Before the merge, `next_fetch_nonce()` and `nonce()` returned the
same value at every `begin_model_fetch` call site, so swapping one for the other was an equivalent
mutation. With the abort attached it is not: the swap strands the previous task, and #56's two
`starting_a_*_fetch_aborts_the_previous_one` tests fail.

### 11.7 Framing: state-space, not line count

`app.rs` shrinks by ~1000 lines but `modal.rs` adds more than that back, so excluding tests this is
net **+152 production lines**. The win this change trades on is state-space, not LOC: `App` fields
44 → 38, one `Option<Modal>` in place of six fields, one draw seam in place of three, one nonce in
place of two, one close path in place of four, and "two modals open at once" unrepresentable rather
than merely unreached. Given ARCHITECTURE.md's note that the predecessor died of surface-area
sprawl, the LOC framing would be the wrong claim as well as an indefensible one.
