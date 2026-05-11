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
