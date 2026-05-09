//! :themes — theme picker with live preview.
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    keys::vim_motion,
    ui::{table::DataTable, theme::{self, Theme}},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct ThemesView {
    id: ViewId,
    table: DataTable,
    saved: &'static Theme,
}

impl ThemesView {
    pub fn new(current: &'static Theme) -> Self {
        let mut table = DataTable::new(vec!["theme"]);
        let rows: Vec<Vec<String>> = theme::ALL.iter()
            .map(|t| vec![t.name.to_string()])
            .collect();
        table.set_rows(rows);
        if let Some(idx) = theme::ALL.iter().position(|t| std::ptr::eq(*t, current)) {
            table.state.select(Some(idx));
        }
        Self { id: ViewId::next(), table, saved: current }
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
