//! :databases — list of databases on the current cluster.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{PgConn, catalog::{DatabaseInfo, list_databases}},
    keys::vim_motion,
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct DatabasesView {
    id: ViewId,
    table: DataTable,
    rows: Vec<DatabaseInfo>,
    error: Option<String>,
    conn: PgConn,
}

impl DatabasesView {
    pub fn new(conn: PgConn) -> Self {
        let mut table = DataTable::new(vec!["name", "owner", "encoding"]);
        table.set_rows(vec![]);
        Self {
            id: ViewId::next(),
            table,
            rows: vec![],
            error: None,
            conn,
        }
    }

    pub fn selected(&self) -> Option<&DatabaseInfo> {
        self.table.selected_index().and_then(|i| self.rows.get(i))
    }
}

impl View for DatabasesView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "databases" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
        // Errors render via the footer toast set by App; nothing here for v1.
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => Outcome::Pass, // App will rebuild conn to selected db (Task 3.6)
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = list_databases(&conn).await;
            let _ = tx.send(AppEvent::ViewData {
                view_id,
                payload: ViewPayload::Databases(result),
            }).await;
        });
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Databases(res) = payload {
            match res {
                Ok(rows) => {
                    self.rows = rows;
                    let display: Vec<Vec<String>> = self.rows.iter().map(|d| vec![
                        d.name.clone(),
                        d.owner.clone(),
                        d.encoding.clone(),
                    ]).collect();
                    self.table.set_rows(display);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e}")),
            }
        }
    }

    fn set_filter(&mut self, filter: &str) { self.table.set_filter(filter); }
    fn supports_filter(&self) -> bool { true }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
