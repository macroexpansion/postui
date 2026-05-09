//! Row detail view: read mode by default; `i` enters edit mode for the
//! selected field; Enter saves (kicks off the mutation flow), Esc cancels.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{
        PgConn,
        catalog::PkColumn,
        mutate::{ColumnEdit, LiteralValue, PrimaryKey, build_insert, build_update},
    },
    ui::{detail::{DetailView, Mode}, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, confirm::ConfirmView},
};

pub struct RowDetailView {
    id: ViewId,
    detail: DetailView,
    conn: PgConn,
    schema: String,
    table: String,
    pk: Vec<PkColumn>,
    insert_mode: bool,
}

impl RowDetailView {
    pub fn new(
        conn: PgConn,
        schema: String,
        table: String,
        pk: Vec<PkColumn>,
        detail: DetailView,
    ) -> Self {
        Self { id: ViewId::next(), detail, conn, schema, table, pk, insert_mode: false }
    }

    pub fn set_insert_mode(&mut self) { self.insert_mode = true; }

    fn current_pk(&self) -> PrimaryKey {
        let cols = self.pk.iter().map(|c| {
            let val = self.detail.fields.iter()
                .find(|f| f.name == c.name)
                .map(|f| LiteralValue::Text(f.original.clone()))
                .unwrap_or(LiteralValue::Null);
            (c.name.clone(), val)
        }).collect();
        PrimaryKey { columns: cols }
    }

    fn submit(&mut self, ctx: &mut Ctx) -> Outcome {
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();

        let sql = if self.insert_mode {
            let edits: Vec<ColumnEdit> = self.detail.fields.iter()
                .filter(|f| !f.edited.is_empty())
                .map(|f| ColumnEdit {
                    name: f.name.clone(),
                    value: LiteralValue::Text(f.edited.clone()),
                })
                .collect();
            match build_insert(&self.schema, &self.table, &edits) {
                Ok(s) => s,
                Err(e) => return Outcome::Push(Box::new(toast_view(format!("can't insert: {e}")))),
            }
        } else {
            let dirty = self.detail.dirty();
            if dirty.is_empty() {
                return Outcome::Consumed;
            }
            let edits: Vec<ColumnEdit> = dirty.iter().map(|f| ColumnEdit {
                name: f.name.clone(),
                value: LiteralValue::Text(f.edited.clone()),
            }).collect();
            let pk = self.current_pk();
            match build_update(&self.schema, &self.table, &edits, &pk) {
                Ok(s) => s,
                Err(e) => return Outcome::Push(Box::new(toast_view(format!("can't update: {e}")))),
            }
        };

        let title = if self.insert_mode { "execute INSERT" } else { "execute UPDATE" };
        let body = if self.insert_mode { "insert this new row?" } else { "this will execute the SQL below." };

        let sql_for_action = sql.clone();
        let confirm = ConfirmView::new(
            title,
            body,
            sql,
            move || {
                let conn = conn.clone();
                let tx = tx.clone();
                let sql = sql_for_action.clone();
                async move {
                    let r = conn.client().execute(sql.as_str(), &[]).await;
                    let toast = match &r {
                        Ok(n) => format!("OK ({n} row(s))"),
                        Err(e) => format!("failed: {e}"),
                    };
                    let _ = tx.send(AppEvent::Toast(toast)).await;
                    r.map(|n| format!("{n}")).map_err(|e| crate::error::DbError::Query {
                        sql: sql.clone(),
                        source: Box::new(e),
                    })
                }
            },
        );
        Outcome::Push(Box::new(confirm))
    }

    fn delete(&mut self, ctx: &mut Ctx) -> Outcome {
        use crate::db::mutate::build_delete;
        let pk = self.current_pk();
        let sql = match build_delete(&self.schema, &self.table, &pk) {
            Ok(s) => s,
            Err(e) => return Outcome::Push(Box::new(toast_view(format!("can't delete: {e}")))),
        };
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();
        let sql_for_action = sql.clone();
        let confirm = ConfirmView::new(
            "execute DELETE",
            "this will permanently delete the row.",
            sql,
            move || {
                let conn = conn.clone();
                let tx = tx.clone();
                let sql = sql_for_action.clone();
                async move {
                    let r = conn.client().execute(sql.as_str(), &[]).await;
                    let toast = match &r {
                        Ok(n) => format!("DELETE {n}"),
                        Err(e) => format!("DELETE failed: {e}"),
                    };
                    let _ = tx.send(AppEvent::Toast(toast)).await;
                    r.map(|n| format!("{n}")).map_err(|e| crate::error::DbError::Query {
                        sql: sql.clone(),
                        source: Box::new(e),
                    })
                }
            },
        );
        Outcome::Push(Box::new(confirm))
    }
}

/// Tiny "view" that just toasts and pops on first key. Used for inline error surfaces.
fn toast_view(msg: String) -> ToastOnce {
    ToastOnce { id: ViewId::next(), msg, shown: false }
}

struct ToastOnce { id: ViewId, msg: String, shown: bool }

impl View for ToastOnce {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "info" }
    fn render(&mut self, _f: &mut Frame, _area: Rect, _t: &Theme) {}
    fn handle_key(&mut self, _key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if !self.shown {
            self.shown = true;
            let _ = ctx.event_tx.try_send(AppEvent::Toast(self.msg.clone()));
        }
        Outcome::Pop
    }
}

impl View for RowDetailView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "row" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.detail.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        match self.detail.mode {
            Mode::View => match key.code {
                KeyCode::Char('j') | KeyCode::Down => { self.detail.move_down(); Outcome::Consumed }
                KeyCode::Char('k') | KeyCode::Up => { self.detail.move_up(); Outcome::Consumed }
                KeyCode::Char('i') => { self.detail.enter_edit(); Outcome::Consumed }
                KeyCode::Char('d') => self.delete(ctx),
                _ => Outcome::Pass,
            }
            Mode::Edit => match key.code {
                KeyCode::Esc => {
                    // Cancel: revert all in-progress edits to originals.
                    for f in &mut self.detail.fields {
                        f.edited.clone_from(&f.original);
                    }
                    self.detail.leave_edit();
                    Outcome::Consumed
                }
                KeyCode::Enter => {
                    self.detail.leave_edit();
                    self.submit(ctx)
                }
                KeyCode::Tab | KeyCode::Down => { self.detail.move_down(); Outcome::Consumed }
                KeyCode::BackTab | KeyCode::Up => { self.detail.move_up(); Outcome::Consumed }
                KeyCode::Backspace => { self.detail.backspace(); Outcome::Consumed }
                KeyCode::Char(c) => { self.detail.append_char(c); Outcome::Consumed }
                _ => Outcome::Consumed,
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}

