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
- The two identical `UiEvent` fetch-result variants collapse to one.
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
collapse to one `begin_model_fetch(&mut self, provider: String, key: Option<String>)`. The two
`UiEvent` variants collapse with them:

```rust
UiEvent::ModelsFetched { nonce: u64, provider: String, result: Result<Vec<String>, String> }
```

`UiEvent::ConnectModels` is deleted and its `run`-loop arm merges into the `ModelsFetched` arm.
`handle_connect_models` + `handle_models_fetched` collapse to one `handle_models_fetched` that
checks the nonce once, then matches the open modal and applies the connect-specific or
models-specific fill. The two fills stay distinct — connect shows a fetch error **inline** in its
model-list step, models falls back to a **manual entry** step — because that difference is real
behaviour, not duplication.

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

The existing tests `closing_the_modal_returns_to_the_mode_it_was_opened_from` and
`help_modal_returns_to_the_mode_it_was_opened_from` are the regression net for this and are kept
verbatim (they assert on `app.mode`, which is now simply never disturbed).

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
2. **`connect_return` / `models_return` / `help_return` are deleted.** Provably inert (§4.5);
   deleting them cannot change behaviour.

Everything else is required to be observably identical, and the moved tests are the check.

## 8. Absorbing the parallel work (#44, #47)

This branch merges after #45, #44 and #47. The design leaves each an obvious landing spot; neither
is implemented here.

- **#44 (`models-fetch-bounds`)** stores the fetch `JoinHandle` and aborts it on close. `ModalHost`
  is exactly that seam: a third private field `fetch: Option<JoinHandle<()>>`, set by
  `begin_model_fetch` (now **one** call site instead of two) and aborted in `ModalHost::close` and
  `ModalHost::open` (now **one** close path instead of four). This is the same shape `leave_engine`
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
5. **Open: `Modal::covers_base()` is a per-variant exception**, not a uniform rule. It is the honest
   encoding of today's behaviour (§2.1). If help is ever restyled as an overlay, the method goes
   away; changing it now would be an unsanctioned behaviour delta.
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
5. **The two `UiEvent` fetch variants merge into one.** Rationale: they are structurally identical
   and now share one `begin_model_fetch`; keeping two would keep the copy the issue objects to.
   `UiEvent` is not a public API (§3, Out).
6. **`apply_and_close_modal` closes before persisting.** Rationale: that is the `/models` ordering
   the issue asks to carry forward, and the orderings are indistinguishable (§4.4).
7. **No i18n change.** Rationale: no string is added, removed, or re-keyed; keys move file but not
   catalog, so `en`/`es` parity is preserved by construction.
8. **Tests move to sit beside the code they cover.** Rationale: repo convention is tests next to
   code; the pure-transition tests follow `connect_step_next`/`models_step_next`/`help_lines` into
   `modal.rs`, while tests that drive an `App` (routing, apply, persistence, rendering) stay in
   `app.rs`. A move is not a deletion — the plan's final step re-counts them.
