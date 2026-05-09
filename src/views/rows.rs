//! Paged rows view (used standalone and embedded in the table inspector).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::{Constraint, Direction, Layout, Rect}, widgets::Paragraph};

use crate::{
    db::{PgConn, rows::{PAGE_SIZE, Page, fetch_page}},
    keys::{Motion, vim_motion},
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct RowsView {
    id: ViewId,
    table: DataTable,
    page: Option<Page>,
    error: Option<String>,
    conn: PgConn,
    schema: String,
    name: String,
    offset: i64,
}

impl RowsView {
    pub fn new(conn: PgConn, schema: String, name: String) -> Self {
        Self {
            id: ViewId::next(),
            table: DataTable::new(vec![]),
            page: None,
            error: None,
            conn,
            schema,
            name,
            offset: 0,
        }
    }

    fn refetch(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let schema = self.schema.clone();
        let name = self.name.clone();
        let offset = self.offset;
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = fetch_page(&conn, &schema, &name, offset).await;
            let _ = tx.send(AppEvent::ViewData {
                view_id,
                payload: ViewPayload::Rows(result),
            }).await;
        });
    }

    /// Build a list of DetailFields for the current selection, marking PK columns.
    pub fn detail_fields(&self, pk_names: &[String]) -> Option<Vec<crate::ui::detail::DetailField>> {
        let page = self.page.as_ref()?;
        let row_idx = self.table.selected_index()?;
        let row = page.rows.get(row_idx)?;
        let pk_set: std::collections::HashSet<&String> = pk_names.iter().collect();
        Some(page.headers.iter().zip(row.iter()).map(|(h, v)| crate::ui::detail::DetailField {
            name: h.clone(),
            original: v.clone(),
            edited: v.clone(),
            is_pk: pk_set.contains(h),
        }).collect())
    }

    pub fn blank_fields(&self, pk_names: &[String]) -> Option<Vec<crate::ui::detail::DetailField>> {
        let page = self.page.as_ref()?;
        let pk_set: std::collections::HashSet<&String> = pk_names.iter().collect();
        Some(page.headers.iter().map(|h| crate::ui::detail::DetailField {
            name: h.clone(),
            original: String::new(),
            edited: String::new(),
            is_pk: pk_set.contains(h),
        }).collect())
    }
}

impl View for RowsView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "rows" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        self.table.render(f, chunks[0], theme);

        let footer = match (&self.page, &self.error) {
            (_, Some(e)) => format!("error: {e}"),
            (Some(p), _) => format!(
                "rows {}–{} (of ~{})",
                p.offset + 1,
                p.offset + p.rows.len() as i64,
                p.estimated_total
            ),
            (None, None) => "loading…".to_string(),
        };
        f.render_widget(Paragraph::new(footer), chunks[1]);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            match m {
                Motion::PageNext | Motion::PageDown => {
                    if let Some(p) = &self.page {
                        if !p.rows.is_empty() && p.rows.len() == PAGE_SIZE as usize {
                            self.offset += PAGE_SIZE;
                            self.refetch(ctx);
                            return Outcome::Consumed;
                        }
                    }
                }
                Motion::PagePrev | Motion::PageUp => {
                    if self.offset > 0 {
                        self.offset = (self.offset - PAGE_SIZE).max(0);
                        self.refetch(ctx);
                        return Outcome::Consumed;
                    }
                }
                _ => {
                    self.table.move_motion(m);
                    return Outcome::Consumed;
                }
            }
        }
        match key.code {
            KeyCode::Enter => Outcome::Pass, // App opens detail view
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.refetch(ctx);
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Rows(res) = payload {
            match res {
                Ok(page) => {
                    self.table = DataTable::new(page.headers.iter().map(String::as_str).collect());
                    self.table.set_rows(page.rows.clone());
                    self.page = Some(page);
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
