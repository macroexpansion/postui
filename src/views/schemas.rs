//! :schemas — list of schemas on the current database.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{
        PgConn,
        catalog::{SchemaInfo, list_schemas},
    },
    keys::Keymap,
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct SchemasView {
    id: ViewId,
    table: DataTable,
    rows: Vec<SchemaInfo>,
    error: Option<String>,
    conn: PgConn,
    keymap: Keymap,
}

impl SchemasView {
    pub fn new(conn: PgConn) -> Self {
        let mut table = DataTable::new(vec!["name", "owner"]);
        table.set_rows(vec![]);
        Self {
            id: ViewId::next(),
            table,
            rows: vec![],
            error: None,
            conn,
            keymap: Keymap::new(),
        }
    }

    pub fn selected(&self) -> Option<&SchemaInfo> {
        self.table.selected_index().and_then(|i| self.rows.get(i))
    }
}

impl View for SchemasView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn title(&self) -> &str {
        "schemas"
    }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        if let Some(m) = self.keymap.handle(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        if self.keymap.is_pending() {
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => Outcome::Pass,
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = list_schemas(&conn).await;
            let _ = tx
                .send(AppEvent::ViewData {
                    view_id,
                    payload: ViewPayload::Schemas(result),
                })
                .await;
        });
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Schemas(res) = payload {
            match res {
                Ok(rows) => {
                    self.rows = rows;
                    let display: Vec<Vec<String>> = self
                        .rows
                        .iter()
                        .map(|s| vec![s.name.clone(), s.owner.clone()])
                        .collect();
                    self.table.set_rows(display);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e}")),
            }
        }
    }

    fn set_filter(&mut self, filter: &str) {
        self.table.set_filter(filter);
    }
    fn supports_filter(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
