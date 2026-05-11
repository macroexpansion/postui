# Themes & Help: Modal Overlay Refactor — Design Spec

**Date:** 2026-05-11
**Status:** Approved (pending written-spec review)

## Motivation

Today `:themes` and `:help` push full-page views onto the view stack. Both
hide the underlying view entirely while open, even though neither represents
a navigation step — they're transient overlays. The user wants `:themes` (and
by extension `:help`) to render as a small modal floating on top of the
current view, with the underlying view still visible.

## Goals

- `:themes` opens an interactive picker as a centered modal overlay. Underlying
  view remains visible behind the modal. Live preview, Enter saves, Esc cancels
  and restores the previous theme.
- `:help` (and the `?` keybinding) opens the help text as a centered modal
  overlay. Any key dismisses.
- Modals are *not* part of the navigation stack. Esc closes the modal but
  never pops a view or quits the app.

## Non-Goals

- Migrating `ConfirmView`, `RowDetailView`, or any other view to the new
  abstraction. `ConfirmView` is a transactional flow with an async action
  callback and stays a `View` for this iteration.
- Filterable themes list. There are five themes; `/` filtering is dropped for
  this iteration.
- Stacking multiple modals. At most one modal is open at a time.

## Architecture

A new `Modal` trait, parallel to `View` but without stack semantics. Lives
alongside `View` in `src/views/mod.rs`.

```rust
pub trait Modal: Send {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> ModalOutcome;
    fn apply(&mut self, _payload: ViewPayload) {}
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    /// Footer hint string shown while this modal is open. Default empty.
    fn hints(&self) -> &str { "" }
}

pub enum ModalOutcome {
    Consumed,  // key handled, keep modal open
    Close,     // close the modal
}
```

`App` gains one field:

```rust
pub modal: Option<Box<dyn Modal>>,
```

Only one modal open at a time. Opening a new modal while one is already open
replaces it (no stacking).

The modal receives the main area from `App::render` and computes its own
centered rect internally, mirroring `ui::confirm::Confirm::render` (uses
`Clear` on its own footprint, so the underlying view shows through everywhere
outside the modal box).

The existing private `centered_rect` helper in `src/ui/confirm.rs` is promoted
to `pub fn` in `src/ui/mod.rs` so `ThemesModal` and `HelpModal` can share it
with `Confirm`. No behavior change for `Confirm`.

## Components

### `src/views/mod.rs`
Add `Modal` trait and `ModalOutcome` enum. No changes to `View`, `ViewId`,
`AppEvent`, or `ViewPayload`.

### `src/views/themes.rs`
Replace `ThemesView` with `ThemesModal`. Same internal state:

```rust
pub struct ThemesModal {
    table: DataTable,
    saved: &'static Theme,
}
```

- Constructor: `ThemesModal::new(current: &'static Theme)` — same as today,
  minus the `ViewId`.
- `Modal::render` — uses `centered_rect` helper (copy or share with
  `ui::confirm`) sized for the theme list (5 rows + title + footer ≈ 9 rows;
  width ~30). Renders `Clear` over its own rect, then the table with a
  bordered block titled "themes".
- `Modal::handle_key` —
  - Motion keys → `table.move_motion(m)` + emit `AppEvent::PreviewTheme`,
    return `Consumed`.
  - Enter → emit `AppEvent::PersistTheme(name)`, return `Close`.
  - Esc → emit `AppEvent::RestoreTheme(self.saved)`, return `Close`.
  - Anything else → `Consumed` (swallow, do not propagate).
- `Modal::hints` — `"[esc] cancel  [enter] save  [↑↓/jk] preview"`.

Drop `set_filter` / `supports_filter` (filtering is out of scope).

### `src/views/help.rs`
Replace `HelpView` with `HelpModal`.

```rust
pub struct HelpModal;
```

- Constructor: `HelpModal::new()`.
- `Modal::render` — `centered_rect` (wider than themes, taller — ~70% × 80%
  of main), renders the existing static `TEXT` constant inside a bordered
  block.
- `Modal::handle_key` — any key returns `Close`.
- `Modal::hints` — `"press any key to dismiss"`.

### `src/app.rs`

**State:** add `pub modal: Option<Box<dyn Modal>>` to `App`. Initialize `None`
in `App::new`.

**Render:** after rendering the top view (or placeholder), paint the modal:

```rust
if let Some(top) = self.views.last_mut() {
    top.render(f, main, self.theme);
} else {
    ui::render_main_placeholder(f, main);
}
if let Some(modal) = self.modal.as_mut() {
    modal.render(f, main, self.theme);
}
```

**Footer hints:** prefer the modal's hints when open:

```rust
let hints = if let Some(m) = self.modal.as_ref() {
    m.hints()
} else if self.views.last().is_some() {
    "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help"
} else {
    "[:] palette  [^Q] quit"
};
```

**Key routing (`handle_key`)** — precedence top → bottom:

1. `Ctrl-Q` quits. Unchanged.
2. Palette mode (`self.palette.open`). Unchanged. (Cannot open `:` while
   modal is open because step 3 swallows the keystroke.)
3. **NEW: Modal mode.** If `self.modal.is_some()`:
   ```rust
   let outcome = modal.handle_key(key, &mut ctx);
   if matches!(outcome, ModalOutcome::Close) { self.modal = None; }
   return;
   ```
4. `:` opens palette. Unchanged. (Suppressed while modal open by step 3.)
5. Filter mode. Unchanged. (Suppressed while modal open by step 3.)
6. `/` opens filter. Unchanged.
7. `?` opens help — changed: `self.modal = Some(Box::new(HelpModal::new()))`.
8. Forward to active view. Unchanged.

**Dispatch (`dispatch_cmd` / `open`):** `Cmd::Open("themes")` and
`Cmd::Open("help")` set `self.modal` instead of calling `self.push(...)`. The
`"themes"` and `"help"` branches move out of `open()` and into a small
`open_modal(verb)` helper (or directly into `dispatch_cmd`).

**Tick:** After the top view's `on_tick`, also tick the modal if open:

```rust
if let Some(top) = self.views.last_mut() {
    top.on_tick(&mut ctx);
}
if let Some(modal) = self.modal.as_mut() {
    modal.on_tick(&mut ctx);
}
```

`AppEvent` handlers (`PreviewTheme`, `RestoreTheme`, `PersistTheme`) are
unchanged — they're handled by `App` directly, not the modal.

### Call sites
- `src/app.rs` — every `HelpView::new()` / `ThemesView::new(...)` push site
  changes to set `self.modal`.
- `src/main.rs` — no construction of `HelpView` / `ThemesView` here; no
  changes expected.

### Tests
- `tests/view_snapshot.rs::help_view_renders_text` — update to construct
  `HelpModal` and call its `render` directly via the `Modal` trait.

## Edge Cases

**Empty view stack + modal open.** `:themes` with no view: placeholder
renders, modal renders on top. Esc closes the modal, placeholder remains.
No quit.

**Esc-sticky-bottom interaction.** The Esc-never-quits rule
(`fix(app): never quit on Esc`) is unaffected. Modals are handled in step 3
of key routing, *before* the view stack code in step 8. A modal's Esc never
reaches stack-pop logic.

**Previous themes-on-empty-stack fix.** Commit
`12c4960 fix(themes): prevent app quit on Enter when themes is the only view`
becomes moot — themes is no longer a stack view. The surviving sticky-bottom
rule covers any remaining edge. No code to revert; the affected branch is
removed naturally when `ThemesView` is deleted.

**Ctrl-Q with modal open.** Ctrl-Q is step 1; it wins over the modal and
quits the app.

**Reopening `:help` or `:themes` while a modal is already open.** Replaces
the existing modal (no stacking). Simple and matches typical TUI behavior.

## Migration Risk

- Removing `View for ThemesView` and `View for HelpView` impls — must remove
  all references / construction sites. `grep` for `ThemesView` and `HelpView`
  catches them.
- The `?` keybinding currently pushes a view; switching to setting a modal
  must happen in lockstep with removing `HelpView`.

## Open Questions

None.

## Out of Scope (Confirmed)

- `ConfirmView` refactor.
- Themes filtering.
- Multiple modals stacked.
- Modal animations or transitions.
