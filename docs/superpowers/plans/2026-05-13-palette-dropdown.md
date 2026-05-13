# Palette Command Dropdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a dropdown popup to the command palette that lists all matching commands, navigable with Ctrl-j/Ctrl-k and arrow keys, selectable with Tab.

**Architecture:** Extend the existing `Palette` struct with a filtered command index list and selection cursor. A `render_dropdown` function draws the popup in the main area (bottom-left, above footer). Key handling in `app.rs` is extended to route navigation keys to palette methods.

**Tech Stack:** Rust, ratatui (List, ListState, Clear, Block), crossterm (KeyCode, KeyModifiers)

---

## File Structure

| File | Responsibility |
|---|---|
| `src/ui/palette.rs` | Data model (`Palette` fields, filtering, navigation, selection), rendering (`render_dropdown`), existing parse/suggest |
| `src/app.rs` | Key routing (Ctrl-j/k, Up/Down to palette), render call for dropdown |

---

### Task 1: Add filtered list and rebuild_filtered to Palette

**Files:**
- Modify: `src/ui/palette.rs:129-165` (Palette struct + impl)
- Tests in: `src/ui/palette.rs` (tests module)

- [ ] **Step 1: Write failing tests for rebuild_filtered**

Add these tests inside the `mod tests` block in `src/ui/palette.rs`:

```rust
#[test]
fn open_populates_all_commands() {
    let mut p = Palette::default();
    p.open();
    assert_eq!(p.filtered.len(), COMMANDS.len());
    assert_eq!(p.selected, 0);
}

#[test]
fn push_filters_by_prefix() {
    let mut p = Palette::default();
    p.open();
    p.push('s');
    let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
    assert!(names.contains(&"schemas"));
    assert!(names.contains(&"sessions"));
    assert!(!names.contains(&"tables"));
    assert_eq!(p.selected, 0);
}

#[test]
fn push_then_backspace_restores_all() {
    let mut p = Palette::default();
    p.open();
    p.push('s');
    assert!(p.filtered.len() < COMMANDS.len());
    p.backspace();
    assert_eq!(p.filtered.len(), COMMANDS.len());
}

#[test]
fn no_match_gives_empty_filtered() {
    let mut p = Palette::default();
    p.open();
    p.push('z');
    assert!(p.filtered.is_empty());
    assert_eq!(p.selected, 0);
}

#[test]
fn space_in_buffer_still_computes_filtered() {
    let mut p = Palette::default();
    p.open();
    for c in "theme ".chars() {
        p.push(c);
    }
    let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
    assert_eq!(names, vec!["theme"]);
}

#[test]
fn prefix_ta_matches_tables_and_terminate() {
    let mut p = Palette::default();
    p.open();
    p.push('t');
    p.push('a');
    let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
    assert!(names.contains(&"tables"));
    assert!(names.contains(&"terminate"));
    assert_eq!(names.len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::palette::tests::open_populates -- --nocapture`
Expected: compilation error — `filtered` field does not exist on `Palette`.

- [ ] **Step 3: Add fields and rebuild_filtered method**

Add `filtered` and `selected` fields to the `Palette` struct. The struct can no longer derive `Default` because we want `open()` to be the only way to populate `filtered`. Replace the derive with a manual impl.

Replace the struct and impl block (lines 129–165) with:

```rust
#[derive(Debug)]
pub struct Palette {
    pub open: bool,
    pub buffer: String,
    pub suggestion: Option<String>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            open: false,
            buffer: String::new(),
            suggestion: None,
            filtered: Vec::new(),
            selected: 0,
        }
    }
}

impl Palette {
    pub fn open(&mut self) {
        self.open = true;
        self.buffer.clear();
        self.suggestion = None;
        self.rebuild_filtered();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
        self.suggestion = None;
        self.filtered.clear();
        self.selected = 0;
    }

    pub fn push(&mut self, c: char) {
        self.buffer.push(c);
        self.suggestion = suggest(&self.buffer);
        self.rebuild_filtered();
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
        self.suggestion = suggest(&self.buffer);
        self.rebuild_filtered();
    }

    pub fn accept_suggestion(&mut self) {
        if let Some(suffix) = self.suggestion.take() {
            self.buffer.push_str(&suffix);
            self.suggestion = suggest(&self.buffer);
            self.rebuild_filtered();
        }
    }

    fn rebuild_filtered(&mut self) {
        let head = match self.buffer.split_once(' ') {
            Some((h, _)) => h,
            None => &self.buffer,
        };
        if head.is_empty() {
            self.filtered = (0..COMMANDS.len()).collect();
        } else {
            self.filtered = COMMANDS
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name.starts_with(head))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::palette`
Expected: all tests PASS (16 existing + 6 new = 22).

- [ ] **Step 5: Commit**

```bash
git add src/ui/palette.rs
git commit -m "feat(palette): add filtered command list to Palette state"
```

---

### Task 2: Add navigation and selection methods

**Files:**
- Modify: `src/ui/palette.rs` (Palette impl)
- Tests in: `src/ui/palette.rs` (tests module)

- [ ] **Step 1: Write failing tests for navigation and selection**

Add these tests inside the `mod tests` block:

```rust
#[test]
fn move_down_advances_selected() {
    let mut p = Palette::default();
    p.open();
    assert_eq!(p.selected, 0);
    p.move_down();
    assert_eq!(p.selected, 1);
}

#[test]
fn move_up_wraps_to_last() {
    let mut p = Palette::default();
    p.open();
    assert_eq!(p.selected, 0);
    p.move_up();
    assert_eq!(p.selected, COMMANDS.len() - 1);
}

#[test]
fn move_down_wraps_to_zero() {
    let mut p = Palette::default();
    p.open();
    p.selected = COMMANDS.len() - 1;
    p.move_down();
    assert_eq!(p.selected, 0);
}

#[test]
fn move_does_nothing_on_empty_filtered() {
    let mut p = Palette::default();
    p.open();
    p.push('z');
    assert!(p.filtered.is_empty());
    p.move_up();
    p.move_down();
    assert_eq!(p.selected, 0);
}

#[test]
fn select_item_fills_buffer() {
    let mut p = Palette::default();
    p.open();
    p.push('q');
    assert_eq!(p.filtered.len(), 1);
    p.select_item();
    assert_eq!(p.buffer, "query");
    let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
    assert_eq!(names, vec!["query"]);
}

#[test]
fn select_item_noop_on_empty_filtered() {
    let mut p = Palette::default();
    p.open();
    p.push('z');
    assert!(p.filtered.is_empty());
    p.select_item();
    assert_eq!(p.buffer, "z");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::palette::tests::move_down_advances -- --nocapture`
Expected: compilation error — `move_down` method does not exist.

- [ ] **Step 3: Add move_up, move_down, select_item methods**

Add these three public methods to the `Palette` impl block, after `accept_suggestion`:

```rust
pub fn move_up(&mut self) {
    if self.filtered.is_empty() {
        return;
    }
    if self.selected == 0 {
        self.selected = self.filtered.len() - 1;
    } else {
        self.selected -= 1;
    }
}

pub fn move_down(&mut self) {
    if self.filtered.is_empty() {
        return;
    }
    if self.selected >= self.filtered.len() - 1 {
        self.selected = 0;
    } else {
        self.selected += 1;
    }
}

pub fn select_item(&mut self) {
    if let Some(&idx) = self.filtered.get(self.selected) {
        self.buffer = COMMANDS[idx].name.to_string();
        self.suggestion = suggest(&self.buffer);
        self.rebuild_filtered();
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::palette`
Expected: all tests PASS (22 previous + 6 new = 28).

- [ ] **Step 5: Commit**

```bash
git add src/ui/palette.rs
git commit -m "feat(palette): add navigation and selection methods"
```

---

### Task 3: Add render_dropdown function

**Files:**
- Modify: `src/ui/palette.rs` (new public function + imports)

- [ ] **Step 1: Add render_dropdown function**

Add imports at the top of `src/ui/palette.rs` (after the existing `//!` comment):

```rust
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use super::theme::Theme;
```

Then add the function after the `Palette` impl block (before `pub enum Cmd`):

```rust
pub fn render_dropdown(f: &mut Frame, area: Rect, theme: &Theme, palette: &Palette) {
    if !palette.open || palette.buffer.contains(' ') || palette.filtered.is_empty() {
        return;
    }

    let max_item_width: usize = palette
        .filtered
        .iter()
        .map(|&i| {
            let cmd = &COMMANDS[i];
            if cmd.aliases.is_empty() {
                cmd.name.len()
            } else {
                cmd.name.len()
                    + 3
                    + cmd
                        .aliases
                        .iter()
                        .map(|a| a.len() + if cmd.aliases.len() > 1 { 2 } else { 0 })
                        .sum::<usize>()
            }
        })
        .max()
        .unwrap_or(0);

    let width = (max_item_width as u16 + 8).min(50).min(area.width);
    let visible = (palette.filtered.len() as u16).min(12);
    let height = visible + 2;
    let y = area.bottom().saturating_sub(height);
    let x = area.x + 1;
    let rect = Rect::new(x, y, width, height);

    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" commands ")
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let items: Vec<ListItem> = palette
        .filtered
        .iter()
        .map(|&i| {
            let cmd = &COMMANDS[i];
            if cmd.aliases.is_empty() {
                ListItem::new(cmd.name.to_string())
            } else {
                use ratatui::text::{Line, Span};
                ListItem::new(Line::from(vec![
                    Span::styled(cmd.name.to_string(), Style::default().fg(theme.fg)),
                    Span::styled(
                        format!(" ({})", cmd.aliases.join(", ")),
                        Style::default().fg(theme.muted),
                    ),
                ]))
            }
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(palette.selected));

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection_bg)
            .fg(theme.selection_fg)
            .add_modifier(Modifier::BOLD),
    ).highlight_symbol("▶ ");

    f.render_stateful_widget(list, inner, &mut state);
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors.

- [ ] **Step 3: Run all palette tests still pass**

Run: `cargo test --lib ui::palette`
Expected: all 28 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui/palette.rs
git commit -m "feat(palette): add render_dropdown for command list popup"
```

---

### Task 4: Wire key handling and rendering in app.rs

**Files:**
- Modify: `src/app.rs:418-433` (palette key block in `handle_key`)
- Modify: `src/app.rs:573-608` (`render` method)

- [ ] **Step 1: Update palette key block in handle_key**

Replace the palette key block (lines 418–433 in `handle_key`) with:

```rust
if self.palette.open {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('j') => {
                self.palette.move_down();
                return;
            }
            KeyCode::Char('k') => {
                self.palette.move_up();
                return;
            }
            _ => {}
        }
    }
    match key.code {
        KeyCode::Esc => self.palette.close(),
        KeyCode::Enter => {
            let cmd = palette::parse(&self.palette.buffer);
            self.palette.close();
            self.toast = None;
            self.dispatch_cmd(cmd);
        }
        KeyCode::Tab => self.palette.select_item(),
        KeyCode::Up => {
            self.palette.move_up();
        }
        KeyCode::Down => {
            self.palette.move_down();
        }
        KeyCode::Backspace => self.palette.backspace(),
        KeyCode::Char(c) => self.palette.push(c),
        _ => {}
    }
    return;
}
```

- [ ] **Step 2: Add render_dropdown call in render method**

In the `render` method, after the main content rendering block and before the modal rendering, add the dropdown call. Insert after line 591 (`modal.render(...)`) and before the hints section:

Replace the section from the modal render through the footer render (lines 589–607) with:

```rust
if let Some(modal) = self.modal.as_mut() {
    modal.render(f, main, self.theme);
}

if self.palette.open {
    palette::render_dropdown(f, main, self.theme, &self.palette);
}

let hints: &str = if let Some(m) = self.modal.as_ref() {
    m.hints()
} else if self.palette.open {
    "[↑↓/C-j/C-k] navigate  [tab] select  [enter] run  [esc] cancel"
} else if self.views.last().is_some() {
    "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help"
} else {
    "[:] palette  [^Q] quit"
};
let toast_or_filter: String;
let footer_text: Option<&str> = if let Some(buf) = &self.filter_input {
    toast_or_filter = format!("/{buf}");
    Some(toast_or_filter.as_str())
} else {
    self.toast.as_deref()
};
footer::render(f, foot, self.theme, hints, footer_text, &self.palette);
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --lib`
Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(palette): wire dropdown key handling and rendering"
```

---

### Task 5: Update existing suggestion tests for new behavior

**Files:**
- Modify: `src/ui/palette.rs` (tests module)

- [ ] **Step 1: Verify no existing tests broke**

Run: `cargo test --lib`
Expected: all tests PASS. The existing suggestion tests (`suggest_*`) test the `suggest()` free function, which is unchanged. The `Palette`-level tests use `open()` which now populates `filtered`, but no existing test constructs `Palette` without `open()` except the `Default` case (which has an empty `filtered`).

- [ ] **Step 2: Add a regression test for full workflow**

Add this test inside the `mod tests` block:

```rust
#[test]
fn full_dropdown_workflow() {
    let mut p = Palette::default();
    p.open();
    assert_eq!(p.filtered.len(), COMMANDS.len());

    p.push('t');
    let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
    assert!(names.contains(&"tables"));
    assert!(names.contains(&"theme"));
    assert!(names.contains(&"themes"));
    assert!(names.contains(&"terminate"));

    p.move_down();
    p.move_down();
    let before = p.selected;
    p.select_item();
    let selected_name = COMMANDS[p.filtered[0]].name;
    assert_eq!(p.buffer, selected_name);
    assert_ne!(before, p.selected);
    assert_eq!(p.selected, 0);

    p.close();
    assert!(!p.open);
    assert!(p.buffer.is_empty());
    assert!(p.filtered.is_empty());
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test --lib`
Expected: all tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui/palette.rs
git commit -m "test(palette): add workflow regression test for dropdown"
```
