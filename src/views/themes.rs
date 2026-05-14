//! :themes — theme picker modal with live preview.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding},
};

use crate::{
    keys::{Keymap, Motion},
    ui::theme::{self, Theme},
    views::{AppEvent, Ctx, Modal, ModalOutcome},
};

/// Fixed-size centered rect (independent of terminal size). Good for a
/// small picker that shouldn't grow with the screen.
fn compact_modal_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

pub struct ThemesModal {
    state: ListState,
    saved: &'static Theme,
    keymap: Keymap,
}

impl ThemesModal {
    pub fn new(current: &'static Theme) -> Self {
        let mut state = ListState::default();
        let idx = theme::ALL
            .iter()
            .position(|t| std::ptr::eq(*t, current))
            .unwrap_or(0);
        state.select(Some(idx));
        Self {
            state,
            saved: current,
            keymap: Keymap::new(),
        }
    }

    fn cursor_theme(&self) -> Option<&'static Theme> {
        self.state
            .selected()
            .and_then(|i| theme::ALL.get(i))
            .copied()
    }

    fn move_motion(&mut self, m: Motion) {
        let len = theme::ALL.len();
        if len == 0 {
            return;
        }
        let last = len - 1;
        let cur = self.state.selected().unwrap_or(0);
        let next = match m {
            Motion::Up => cur.saturating_sub(1),
            Motion::Down => (cur + 1).min(last),
            Motion::Home | Motion::PageUp | Motion::PagePrev => 0,
            Motion::End | Motion::PageDown | Motion::PageNext => last,
            Motion::Left | Motion::Right => cur,
        };
        self.state.select(Some(next));
    }

    fn preview(&self, ctx: &mut Ctx) {
        if let Some(t) = self.cursor_theme() {
            let _ = ctx.event_tx.try_send(AppEvent::PreviewTheme(t));
        }
    }
}

impl Modal for ThemesModal {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let rect = compact_modal_rect(60, 20, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" themes ")
            .padding(Padding::symmetric(2, 1))
            .border_style(Style::default().fg(theme.border));
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let items: Vec<ListItem> = theme::ALL.iter().map(|t| ListItem::new(t.name)).collect();
        let list = List::new(items).highlight_style(
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, inner, &mut self.state);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> ModalOutcome {
        if let Some(m) = self.keymap.handle(key) {
            self.move_motion(m);
            self.preview(ctx);
            return ModalOutcome::Consumed;
        }
        if self.keymap.is_pending() {
            return ModalOutcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => {
                if let Some(t) = self.cursor_theme() {
                    let _ = ctx
                        .event_tx
                        .try_send(AppEvent::PersistTheme(t.name.to_string()));
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

    fn hints(&self) -> &str {
        "[esc] cancel  [enter] save  [↑↓/jk] preview"
    }
}
