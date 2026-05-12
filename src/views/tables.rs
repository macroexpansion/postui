//! :tables — list of tables in the current schema.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{
        PgConn,
        catalog::{TableInfo, list_tables},
    },
    keys::vim_motion,
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct TablesView {
    id: ViewId,
    table: DataTable,
    rows: Vec<TableInfo>,
    error: Option<String>,
    conn: PgConn,
    schema: String,
}

impl TablesView {
    pub fn new(conn: PgConn, schema: String) -> Self {
        let mut table = DataTable::new(vec!["name", "rows", "size"]);
        table.set_rows(vec![]);
        Self {
            id: ViewId::next(),
            table,
            rows: vec![],
            error: None,
            conn,
            schema,
        }
    }

    pub fn selected(&self) -> Option<&TableInfo> {
        self.table.selected_index().and_then(|i| self.rows.get(i))
    }
}

fn human_bytes(bytes: i64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        return format!("{bytes} B");
    }
    if b < K * K {
        return format!("{:.1} KB", b / K);
    }
    if b < K * K * K {
        return format!("{:.1} MB", b / K / K);
    }
    format!("{:.2} GB", b / K / K / K)
}

impl View for TablesView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn title(&self) -> &str {
        "tables"
    }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => Outcome::Pass, // App pushes inspector (M4)
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let schema = self.schema.clone();
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = list_tables(&conn, &schema).await;
            let _ = tx
                .send(AppEvent::ViewData {
                    view_id,
                    payload: ViewPayload::Tables(result),
                })
                .await;
        });
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Tables(res) = payload {
            match res {
                Ok(rows) => {
                    self.rows = rows;
                    let display: Vec<Vec<String>> = self
                        .rows
                        .iter()
                        .map(|t| {
                            vec![
                                t.name.clone(),
                                t.estimated_rows.to_string(),
                                human_bytes(t.total_bytes),
                            ]
                        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_renders() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
