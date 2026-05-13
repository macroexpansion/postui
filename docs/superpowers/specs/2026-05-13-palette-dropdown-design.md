# Palette Command Dropdown

## Problem

The palette (`:`) currently shows only the first matching command as ghost text via `suggest()`. Users must know command names or type enough characters to get the right suggestion. There is no way to browse all available commands.

## Solution

Add a dropdown popup above the footer that lists all commands, filtered by the user's typed input. The user navigates the list with Ctrl-j/Ctrl-k and Up/Down arrows, selects with Tab (fills buffer), and executes with Enter.

## Data Model

Two new fields on `Palette`:

```rust
pub struct Palette {
    pub open: bool,
    pub buffer: String,
    pub suggestion: Option<String>,
    filtered: Vec<usize>,   // indices into COMMANDS
    selected: usize,        // cursor position within filtered
}
```

- `filtered` is recomputed on every buffer change (`open`, `push`, `backspace`, `select_item`).
- Filtering logic: extract the first word from the buffer (before any space). Match all `COMMANDS` where `name.starts_with(head)`. When the palette opens with an empty buffer, all commands are shown.
- `selected` resets to 0 whenever `filtered` changes.

New methods on `Palette`:

- `rebuild_filtered()` — recompute `filtered` from current buffer, reset `selected` to 0.
- `move_up()` — decrement `selected`, wrapping to last item.
- `move_down()` — increment `selected`, wrapping to first item.
- `select_item()` — fill `buffer` with the selected command's name, recompute suggestion and filtered list.

## Rendering

A `render_dropdown(f, main_area, theme, palette)` function draws the popup in the main content area, positioned at bottom-left just above the footer.

Positioning:
- Width: `max(name.len() + alias_display.len()) + padding`, capped at ~40 chars.
- Height: `min(filtered.len(), 12)`.
- X: left-aligned with a small indent (x = main_area.x + 1).
- Y: `main_area.bottom() - height`.

Visual:
- `Clear` the rect, then a `Block` with `Borders::ALL` and title `" commands "`.
- Render a `List` using `ListState` tracking `selected`.
- `highlight_style` uses theme selection colors and `highlight_symbol("▶ ")`.
- Each item shows command name; aliases shown dimmed after the name (e.g., `databases (db)`).

Called from `app.rs::render()` after main content, before footer:

```rust
if self.palette.open {
    palette::render_dropdown(f, main, self.theme, &self.palette);
}
footer::render(f, foot, ...);
```

## Key Handling

In the palette key block of `app.rs::handle_key()`, add Ctrl modifier check before the existing `key.code` match:

```rust
if self.palette.open {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('j') => { self.palette.move_down(); return; }
            KeyCode::Char('k') => { self.palette.move_up(); return; }
            _ => {}
        }
    }
    match key.code {
        KeyCode::Up => self.palette.move_up(),
        KeyCode::Down => self.palette.move_down(),
        KeyCode::Tab => self.palette.select_item(),
        KeyCode::Esc => self.palette.close(),
        KeyCode::Enter => { /* parse & dispatch, unchanged */ }
        KeyCode::Backspace => self.palette.backspace(),
        KeyCode::Char(c) => self.palette.push(c),
        _ => {}
    }
    return;
}
```

Behavior summary:
- **Ctrl-j / Down** — move selection down (wraps)
- **Ctrl-k / Up** — move selection up (wraps)
- **Tab** — fill buffer with selected command name
- **Enter** — execute whatever is in the buffer (unchanged)
- **Esc** — close palette (unchanged)
- **Typing** — filters the list, resets selection to 0

## Files Changed

1. `src/ui/palette.rs` — add `filtered`, `selected` fields; add `rebuild_filtered`, `move_up`, `move_down`, `select_item` methods; add `render_dropdown` function; update existing methods to call `rebuild_filtered`.
2. `src/app.rs` — update palette key block for Ctrl-j/k and Up/Down; call `render_dropdown` in `render()`.
3. `src/ui/palette.rs` tests — update existing tests that construct `Palette` (now non-trivial `Default`), add tests for filtering, navigation, and selection.

## Edge Cases

- Empty buffer → show all commands.
- No matches → dropdown shows empty list (or is hidden).
- Buffer contains a space (user typed a command + argument) → dropdown is not rendered (`render_dropdown` checks for no-space). `filtered` still holds the single matching command but the popup is hidden to avoid visual noise while typing arguments.
- Backspace to empty → all commands reappear.
- Selection wraps: moving up from index 0 goes to last item, moving down from last goes to index 0.
