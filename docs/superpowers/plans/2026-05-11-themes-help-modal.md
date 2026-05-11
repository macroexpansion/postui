# Themes & Help Modal Overlay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `:themes` and `:help` out of the navigation view stack into a new `Modal` abstraction that renders on top of the current view, keeping the underlying view visible.

**Architecture:** A new `Modal` trait parallel to `View` (no id, no title, no Push/Pop) lives in `src/views/mod.rs`. `App` gains a single `modal: Option<Box<dyn Modal>>` slot. `App::render` paints the top view as before, then paints the modal on top if set. `App::handle_key` routes to the modal *before* the view stack (after Ctrl-Q and palette mode). `ThemesModal` and `HelpModal` replace `ThemesView` and `HelpView` respectively. `ConfirmView` is out of scope.

**Tech Stack:** Rust 2024, ratatui 0.30, crossterm 0.29, tokio. Existing test pattern: `ratatui::backend::TestBackend` for render-buffer assertions in `tests/view_snapshot.rs`.

**Spec:** `docs/superpowers/specs/2026-05-11-themes-help-modal-design.md`.

---

## File Structure

**New / modified:**
- `src/ui/mod.rs` — add `pub fn centered_rect` (promoted from `ui/confirm.rs`).
- `src/ui/confirm.rs` — drop private `centered_rect`; use shared one.
- `src/views/mod.rs` — add `Modal` trait + `ModalOutcome` enum.
- `src/views/help.rs` — replace `HelpView` with `HelpModal`.
- `src/views/themes.rs` — replace `ThemesView` with `ThemesModal`.
- `src/app.rs` — add `modal` field, render hook, key routing, tick hook, dispatch changes.
- `tests/view_snapshot.rs` — replace `help_view_renders_text` with `help_modal_renders_text`; add `themes_modal_renders_table`.

No new modules are created. The Modal trait shares the file `src/views/mod.rs` with `View` because the two concepts are intentionally adjacent.

---

## Task 1: Promote `centered_rect` to shared helper

**Files:**
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/confirm.rs:45-63`

Pure refactor. No new tests; the existing test suite is the contract.

- [ ] **Step 1: Verify baseline is clean**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: build OK, all 61 unit tests + `help_view_renders_text` pass.

- [ ] **Step 2: Add the public helper to `src/ui/mod.rs`**

Append at the end of `src/ui/mod.rs`:

```rust
/// Centered rect with the given percentage of `r`'s width and height.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: ratatui::layout::Rect) -> ratatui::layout::Rect {
    use ratatui::layout::{Constraint, Direction, Layout};
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
```

- [ ] **Step 3: Delete the private `centered_rect` from `src/ui/confirm.rs`**

Remove lines 45-63 in `src/ui/confirm.rs` (the private `fn centered_rect`).

- [ ] **Step 4: Update the call site in `confirm.rs`**

In `src/ui/confirm.rs::Confirm::render` (around line 20), change:

```rust
let modal = centered_rect(70, 50, area);
```

to:

```rust
let modal = crate::ui::centered_rect(70, 50, area);
```

- [ ] **Step 5: Build + test**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: build OK, all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs src/ui/confirm.rs
git commit -m "refactor(ui): promote centered_rect to a shared helper

No behavior change. Modals introduced in the next commits will share
this helper with ConfirmView.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add `Modal` trait + `ModalOutcome` enum

**Files:**
- Modify: `src/views/mod.rs`

Type definitions only. No call sites yet; `cargo build` is the validation.

- [ ] **Step 1: Add types to `src/views/mod.rs`**

Append after the `View` trait (after line 138 in the current file):

```rust
/// A transient overlay rendered on top of the active view. Unlike `View`,
/// modals do not participate in navigation: they have no id, no title, and
/// cannot push/pop. They live in `App::modal: Option<Box<dyn Modal>>` and
/// are routed before the view stack.
pub trait Modal: Send {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> ModalOutcome;
    fn apply(&mut self, _payload: ViewPayload) {}
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    /// Footer hint string shown while this modal is open.
    fn hints(&self) -> &str { "" }
}

pub enum ModalOutcome {
    /// Key handled, keep the modal open.
    Consumed,
    /// Close the modal.
    Close,
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: clean build (dead_code lint may warn about unused trait — that's fine; lints are warn-only).

- [ ] **Step 3: Commit**

```bash
git add src/views/mod.rs
git commit -m "feat(views): add Modal trait + ModalOutcome enum

Parallel to View but without stack semantics: no id, no title, no
Push/Pop. Will host ThemesModal and HelpModal in following commits.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Implement `HelpModal` (TDD)

**Files:**
- Modify: `src/views/help.rs` (add `HelpModal` alongside existing `HelpView`)
- Modify: `tests/view_snapshot.rs` (add `help_modal_renders_text` test)

`HelpView` is **not deleted** in this task — it stays so the app still compiles. Task 5 deletes it after the App is wired.

- [ ] **Step 1: Write the failing test**

Append to `tests/view_snapshot.rs`:

```rust
#[test]
fn help_modal_renders_text() {
    use postui::views::{help::HelpModal, Modal, Ctx};
    let backend = TestBackend::new(80, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut help = HelpModal::new();
    let theme = &theme::DEFAULT;
    let (_tx, _rx) = mpsc::channel(8);
    let _ctx = Ctx::new(_tx);

    term.draw(|f| {
        let area = Rect::new(0, 0, 80, 30);
        help.render(f, area, theme);
    }).unwrap();

    let buf = term.backend().buffer();
    let dump = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(dump.contains("postui"));
    assert!(dump.contains("palette"));
    assert!(dump.contains(":query"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test view_snapshot help_modal_renders_text`
Expected: FAIL with `unresolved import postui::views::help::HelpModal`.

- [ ] **Step 3: Implement `HelpModal` in `src/views/help.rs`**

Replace the contents of `src/views/help.rs` with:

```rust
//! :help — modal listing keybindings and palette commands.

use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    ui::{self, theme::Theme},
    views::{Ctx, Modal, ModalOutcome, Outcome, View, ViewId},
};

const TEXT: &str = "
  postui — quick help

  Universal:
    :              open palette       /              filter visible rows
    Esc            pop view           ^Q             quit
    ^C             cancel in-flight   ?              this help

  Movement (lists):
    j  k           down  up           h  l           tab switch / left / right
    w  b           page down / up     e              jump to last row

  :query editor:
    ^R | F5        run                ^E             open in $EDITOR
    ^N | ^P        next / prev result tab

  Activity:
    ^K             cancel selected backend (pg_cancel_backend)
    :terminate <pid>  forcefully terminate (pg_terminate_backend)

  Palette commands:
    :connections  :databases (:db)  :schemas (:sc)  :tables (:tb)
    :query (:sql) :queries  :locks  :sessions
    :themes  :theme <name>
    :connect [uri-or-name]
    :terminate <pid>
    :q | :quit

  Row editing:
    Enter          drill in (or open row detail)
    i              edit selected (in row detail) or current field
    a              insert a new row
    d              delete selected row
    Enter          submit edit (in edit mode)  Esc cancel

  Press any key to dismiss.
";

fn render_text(f: &mut Frame, area: Rect, theme: &Theme) {
    let rect = ui::centered_rect(70, 80, area);
    f.render_widget(Clear, rect);
    let p = Paragraph::new(TEXT)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
        .wrap(Wrap { trim: false });
    f.render_widget(p, rect);
}

// --- Modal (new) ---

pub struct HelpModal;

impl HelpModal {
    pub fn new() -> Self { Self }
}

impl Default for HelpModal { fn default() -> Self { Self::new() } }

impl Modal for HelpModal {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        render_text(f, area, theme);
    }

    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut Ctx) -> ModalOutcome {
        ModalOutcome::Close
    }

    fn hints(&self) -> &str { "press any key to dismiss" }
}

// --- View (deprecated; deleted in a follow-up task) ---

pub struct HelpView { id: ViewId }

impl HelpView {
    pub fn new() -> Self { Self { id: ViewId::next() } }
}

impl Default for HelpView { fn default() -> Self { Self::new() } }

impl View for HelpView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "help" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        render_text(f, area, theme);
    }

    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        Outcome::Pop
    }
}
```

Note: `HelpView`'s render is rewritten to call `render_text`, which now uses a centered rect. That's a slight visual change (help becomes a centered box even before the App is wired), but it has no user-facing effect because `HelpView` is removed in Task 5 anyway. Sharing `render_text` avoids duplication.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test view_snapshot help_modal_renders_text`
Expected: PASS.

- [ ] **Step 5: Run the full test suite as a regression check**

Run: `cargo test --lib --bins --test view_snapshot`
Expected: all tests pass, including the original `help_view_renders_text`.

- [ ] **Step 6: Commit**

```bash
git add src/views/help.rs tests/view_snapshot.rs
git commit -m "feat(views): add HelpModal alongside HelpView

HelpModal implements the new Modal trait. HelpView is unchanged
externally; both share render_text(). HelpView will be removed once
the App is wired to use the modal slot.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Wire `App::modal` slot, render hook, key routing, tick hook, footer hints

**Files:**
- Modify: `src/app.rs`

No tests; the integration is exercised when modals are migrated in Tasks 5 and 7. `cargo build` is the validation here.

- [ ] **Step 1: Import `Modal` and `ModalOutcome` in `app.rs`**

In the `use crate::{...}` block at the top of `src/app.rs` (line 10-16), update the `views` import:

```rust
use crate::{
    db::PgConn,
    error::Result,
    term::Tui,
    ui::{self, footer, header, palette::{self, Cmd, Palette}, theme::{self, Theme}},
    views::{AppEvent, Ctx, Modal, ModalOutcome, Outcome, View},
};
```

- [ ] **Step 2: Add `modal` field to `App`**

In the `pub struct App { ... }` block (around line 21-36), add the field after `filter_input`:

```rust
    /// Active modal overlay (themes picker, help, etc.). At most one open at
    /// a time. Routed before the view stack in `handle_key`.
    pub modal: Option<Box<dyn Modal>>,
```

- [ ] **Step 3: Initialize `modal: None` in `App::new`**

In `App::new` (around line 39-56), add `modal: None,` to the struct literal (after `filter_input`).

- [ ] **Step 4: Add modal render hook in `App::render`**

Find `App::render` (around line 501-529). After the view-render block:

```rust
        if let Some(top) = self.views.last_mut() {
            top.render(f, main, self.theme);
        } else {
            ui::render_main_placeholder(f, main);
        }
```

Append:

```rust
        if let Some(modal) = self.modal.as_mut() {
            modal.render(f, main, self.theme);
        }
```

- [ ] **Step 5: Prefer modal hints in the footer**

In `App::render`, find the `hints` assignment (around line 517-520):

```rust
        let hints = match self.views.last() {
            Some(_) => "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help",
            None => "[:] palette  [^Q] quit",
        };
```

Replace with:

```rust
        let hints: &str = if let Some(m) = self.modal.as_ref() {
            m.hints()
        } else if self.views.last().is_some() {
            "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help"
        } else {
            "[:] palette  [^Q] quit"
        };
```

- [ ] **Step 6: Route keys to the modal first**

In `App::handle_key` (starts around line 353), insert a new block **immediately after** the palette-mode block (which ends around line 376 with `return;` inside `if self.palette.open { ... }`) and **before** the `if key.code == KeyCode::Char(':')` palette-open block.

Insert:

```rust
        // modal owns the keys until closed; Ctrl-Q above still wins.
        if let Some(modal) = self.modal.as_mut() {
            let mut ctx = Ctx::new(self.event_tx.clone());
            let outcome = modal.handle_key(key, &mut ctx);
            if matches!(outcome, ModalOutcome::Close) {
                self.modal = None;
            }
            return;
        }
```

- [ ] **Step 7: Tick the modal alongside the top view**

In `App::run` (around line 558-562), find the tick branch:

```rust
                _ = tick.tick() => {
                    let mut ctx = Ctx::new(self.event_tx.clone());
                    if let Some(top) = self.views.last_mut() {
                        top.on_tick(&mut ctx);
                    }
                }
```

Replace with:

```rust
                _ = tick.tick() => {
                    let mut ctx = Ctx::new(self.event_tx.clone());
                    if let Some(top) = self.views.last_mut() {
                        top.on_tick(&mut ctx);
                    }
                    if let Some(modal) = self.modal.as_mut() {
                        modal.on_tick(&mut ctx);
                    }
                }
```

- [ ] **Step 8: Build + test**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: clean build, all tests pass. Nothing sets `self.modal` yet, so behavior is unchanged.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add modal slot, render/key/tick hooks, footer hints

Plumbs the Modal trait into App without yet using it. ThemesView and
HelpView still drive :themes / :help. Modal routing happens after Ctrl-Q
and palette mode, before the view stack.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Migrate Help to the modal slot; delete `HelpView`

**Files:**
- Modify: `src/app.rs`
- Modify: `src/views/help.rs`
- Modify: `tests/view_snapshot.rs`

- [ ] **Step 1: Update the `?` keybinding in `App::handle_key`**

In `src/app.rs`, find the help-open branch (around line 418-422):

```rust
        // open help modal
        if key.code == KeyCode::Char('?') {
            self.push(Box::new(crate::views::help::HelpView::new()));
            return;
        }
```

Replace with:

```rust
        // open help modal
        if key.code == KeyCode::Char('?') {
            self.modal = Some(Box::new(crate::views::help::HelpModal::new()));
            return;
        }
```

- [ ] **Step 2: Update `:help` in `App::open`**

In `src/app.rs::open()` (around line 157-160), find:

```rust
            "help" => {
                self.push(Box::new(crate::views::help::HelpView::new()));
                return;
            }
```

Replace with:

```rust
            "help" => {
                self.modal = Some(Box::new(crate::views::help::HelpModal::new()));
                return;
            }
```

- [ ] **Step 3: Delete `HelpView` from `src/views/help.rs`**

Remove the `// --- View (deprecated; deleted in a follow-up task) ---` section and everything below it from `src/views/help.rs`. Also remove the now-unused imports from the top of the file: `Outcome`, `View`, `ViewId` should disappear from the `crate::views::{...}` import.

The final `src/views/help.rs` import block should be:

```rust
use crate::{
    ui::{self, theme::Theme},
    views::{Ctx, Modal, ModalOutcome},
};
```

- [ ] **Step 4: Delete the obsolete `help_view_renders_text` test**

In `tests/view_snapshot.rs`, delete the entire `help_view_renders_text` test function (the original one, leaving `help_modal_renders_text` from Task 3).

Also clean the imports in `tests/view_snapshot.rs`. The remaining test only needs:

```rust
use postui::ui::theme;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use tokio::sync::mpsc;
```

(The `HelpModal`, `Modal`, `Ctx` imports stay inside `help_modal_renders_text` as local `use` statements.)

- [ ] **Step 5: Build + test**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: clean build, all tests pass. `cargo build` warnings about unused items should be zero.

- [ ] **Step 6: Manual smoke check (optional but recommended)**

Launch postui (e.g., `cargo run -- --uri "postgres://..."` if you have one handy, or just `cargo run` to land on the connections picker), then:
- Press `?` — help appears as a centered modal box; underlying view visible behind.
- Press any key — modal closes.
- Type `:help<Enter>` — same.
- With a modal open, type `:` — palette does NOT open (modal swallows it). Press a key to close the modal first.
- With a modal open, press Ctrl-Q — app quits.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/views/help.rs tests/view_snapshot.rs
git commit -m "refactor(help): :help and ? now open a modal overlay

HelpView is removed; HelpModal is wired into App::modal. Underlying
view stays visible behind the help box.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Implement `ThemesModal` (TDD)

**Files:**
- Modify: `src/views/themes.rs` (add `ThemesModal` alongside existing `ThemesView`)
- Modify: `tests/view_snapshot.rs` (add `themes_modal_renders_table` test)

`ThemesView` stays in this task; deleted in Task 7.

- [ ] **Step 1: Write the failing test**

Append to `tests/view_snapshot.rs`:

```rust
#[test]
fn themes_modal_renders_table() {
    use postui::views::{themes::ThemesModal, Modal};
    let backend = TestBackend::new(80, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut modal = ThemesModal::new(&theme::DEFAULT);
    let theme = &theme::DEFAULT;

    term.draw(|f| {
        let area = Rect::new(0, 0, 80, 30);
        modal.render(f, area, theme);
    }).unwrap();

    let buf = term.backend().buffer();
    let dump = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(dump.contains("themes"), "expected block title 'themes'");
    assert!(dump.contains("default"), "expected 'default' theme name in list");
    assert!(dump.contains("dracula"), "expected 'dracula' theme name in list");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test view_snapshot themes_modal_renders_table`
Expected: FAIL with `unresolved import postui::views::themes::ThemesModal`.

- [ ] **Step 3: Implement `ThemesModal` in `src/views/themes.rs`**

Replace the contents of `src/views/themes.rs` with:

```rust
//! :themes — theme picker modal with live preview.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear},
};

use crate::{
    keys::vim_motion,
    ui::{table::DataTable, theme::{self, Theme}},
    views::{AppEvent, Ctx, Modal, ModalOutcome, Outcome, View, ViewId, ViewPayload},
};

fn build_table(current: &'static Theme) -> DataTable {
    let mut table = DataTable::new(vec!["theme"]);
    let rows: Vec<Vec<String>> = theme::ALL.iter()
        .map(|t| vec![t.name.to_string()])
        .collect();
    table.set_rows(rows);
    if let Some(idx) = theme::ALL.iter().position(|t| std::ptr::eq(*t, current)) {
        table.state.select(Some(idx));
    }
    table
}

/// Fixed-size centered rect (independent of terminal size). Good for a
/// small picker that shouldn't grow with the screen.
fn compact_modal_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

// --- Modal (new) ---

pub struct ThemesModal {
    table: DataTable,
    saved: &'static Theme,
}

impl ThemesModal {
    pub fn new(current: &'static Theme) -> Self {
        Self { table: build_table(current), saved: current }
    }

    fn cursor_theme(&self) -> Option<&'static Theme> {
        self.table.selected_index().and_then(|i| theme::ALL.get(i)).copied()
    }

    fn preview(&self, ctx: &mut Ctx) {
        if let Some(t) = self.cursor_theme() {
            let _ = ctx.event_tx.try_send(AppEvent::PreviewTheme(t));
        }
    }
}

impl Modal for ThemesModal {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        // 5 themes + 1 header + 2 borders = 8 rows; width 30 fits longest name + padding.
        let rect = compact_modal_rect(30, 8, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" themes ")
            .border_style(Style::default().fg(theme.border));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        self.table.render(f, inner, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> ModalOutcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            self.preview(ctx);
            return ModalOutcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => {
                if let Some(t) = self.cursor_theme() {
                    let _ = ctx.event_tx.try_send(AppEvent::PersistTheme(t.name.to_string()));
                }
                ModalOutcome::Close
            }
            KeyCode::Esc => {
                let _ = ctx.event_tx.try_send(AppEvent::RestoreTheme(self.saved));
                ModalOutcome::Close
            }
            _ => ModalOutcome::Consumed,
        }
    }

    fn hints(&self) -> &str { "[esc] cancel  [enter] save  [↑↓/jk] preview" }
}

// --- View (deprecated; deleted in a follow-up task) ---

pub struct ThemesView {
    id: ViewId,
    table: DataTable,
    saved: &'static Theme,
}

impl ThemesView {
    pub fn new(current: &'static Theme) -> Self {
        Self { id: ViewId::next(), table: build_table(current), saved: current }
    }

    fn cursor_theme(&self) -> Option<&'static Theme> {
        self.table.selected_index().and_then(|i| theme::ALL.get(i)).copied()
    }

    fn preview(&self, ctx: &mut Ctx) {
        if let Some(t) = self.cursor_theme() {
            let _ = ctx.event_tx.try_send(AppEvent::PreviewTheme(t));
        }
    }
}

impl View for ThemesView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "themes" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            self.preview(ctx);
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => {
                if let Some(t) = self.cursor_theme() {
                    let _ = ctx.event_tx.try_send(AppEvent::PersistTheme(t.name.to_string()));
                }
                Outcome::Pop
            }
            KeyCode::Esc => {
                let _ = ctx.event_tx.try_send(AppEvent::RestoreTheme(self.saved));
                Outcome::Pop
            }
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.preview(ctx);
    }

    fn apply(&mut self, _payload: ViewPayload) {}

    fn set_filter(&mut self, filter: &str) { self.table.set_filter(filter); }
    fn supports_filter(&self) -> bool { true }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test view_snapshot themes_modal_renders_table`
Expected: PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib --bins --test view_snapshot`
Expected: all tests pass (including `help_modal_renders_text` and `themes_modal_renders_table`).

- [ ] **Step 6: Commit**

```bash
git add src/views/themes.rs tests/view_snapshot.rs
git commit -m "feat(views): add ThemesModal alongside ThemesView

ThemesModal implements the new Modal trait with the same live-preview /
save-on-Enter / restore-on-Esc behavior as ThemesView. Both share
build_table() to keep them in sync. ThemesView will be removed once
the App is wired to use the modal slot.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Migrate `:themes` to the modal slot; delete `ThemesView`

**Files:**
- Modify: `src/app.rs`
- Modify: `src/views/themes.rs`

- [ ] **Step 1: Update `:themes` in `App::open`**

In `src/app.rs::open()` (around line 152-156), find:

```rust
            "themes" => {
                use crate::views::themes::ThemesView;
                self.push(Box::new(ThemesView::new(self.theme)));
                return;
            }
```

Replace with:

```rust
            "themes" => {
                use crate::views::themes::ThemesModal;
                self.modal = Some(Box::new(ThemesModal::new(self.theme)));
                return;
            }
```

- [ ] **Step 2: Delete `ThemesView` from `src/views/themes.rs`**

Remove the `// --- View (deprecated; deleted in a follow-up task) ---` section and everything below it from `src/views/themes.rs`. Then clean the now-unused imports at the top of the file. The final import block should be:

```rust
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear},
};

use crate::{
    keys::vim_motion,
    ui::{table::DataTable, theme::{self, Theme}},
    views::{AppEvent, Ctx, Modal, ModalOutcome},
};
```

(Drop `Outcome, View, ViewId, ViewPayload` — not used by `ThemesModal`.)

- [ ] **Step 3: Build + test**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: clean build with zero warnings, all tests pass.

- [ ] **Step 4: Manual smoke check**

Launch postui. Verify:
- `:themes<Enter>` — small centered picker appears over the current view (or placeholder); underlying view visible behind.
- `j`/`k` move the cursor — the entire screen previews the highlighted theme (both the modal and the view behind).
- `Enter` — modal closes, theme is saved (toast: `theme: <name> (saved)`).
- `Esc` — modal closes, theme reverts to the one selected when the picker opened.
- `?` while themes is open — has no effect (themes-modal swallows `?`). Close themes first.
- Open `:themes`, then while open type `:` — palette does NOT open.
- Ctrl-Q at any point quits.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/views/themes.rs
git commit -m "refactor(themes): :themes now opens a modal overlay

ThemesView is removed; ThemesModal is wired into App::modal. Underlying
view stays visible behind the picker box; theme previews live-repaint
both layers.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Final verification

**Files:**
- None (verification only).

- [ ] **Step 1: Full build + test**

Run: `cargo build && cargo test --lib --bins --test view_snapshot`
Expected: clean build, all unit tests pass, both `help_modal_renders_text` and `themes_modal_renders_table` pass.

- [ ] **Step 2: Clippy / lint sanity (best-effort)**

Run: `cargo clippy --all-targets -- -D warnings` (allowed to fail if the existing codebase has pre-existing clippy noise; treat new warnings introduced by this work as bugs).
Expected: no new warnings in `src/views/themes.rs`, `src/views/help.rs`, `src/views/mod.rs`, `src/app.rs`, `src/ui/mod.rs`, or `tests/view_snapshot.rs`.

- [ ] **Step 3: Confirm git history is clean**

Run: `git log --oneline origin/main..HEAD`
Expected: 7 new commits (one per Task 1–7); no fixup or stash entries.

---

## Out of Scope (Confirmed)

- `ConfirmView` refactor. Stays a `View`.
- Themes filtering (`/` while picker open). Dropped.
- Multiple modals stacked.
- Modal animations.

## Notes for the Implementing Engineer

- The `ViewPayload` import in the Modal trait default `apply` body is referenced only as a type in the signature; no need to use it in any modal in this plan.
- `DataTable::render` already handles selection highlight and filter state. `ThemesModal` doesn't call `set_filter`, so the table behaves like a static list.
- The Esc-never-quits rule from `fix(app): never quit on Esc; bottom view is sticky` is **not** changed by this plan. Modal Esc handling happens before view-stack Esc handling, so there is no interaction.
- Commit `12c4960 fix(themes): prevent app quit on Enter when themes is the only view` becomes moot when `ThemesView` is deleted. No revert is needed — the affected code path is removed naturally.
