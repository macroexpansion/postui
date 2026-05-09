//! :queries / :locks / :sessions — live polling views.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};
use tokio::{select, time::interval};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{
        PgConn,
        activity::{ActivityFilter, ActivityRow, LockRow, activity, cancel_backend, locks},
    },
    keys::vim_motion,
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind { Queries, Locks, Sessions }

pub struct ActivityView {
    id: ViewId,
    kind: ActivityKind,
    table: DataTable,
    rows: Vec<ActivityRow>,
    locks: Vec<LockRow>,
    poll_token: Option<CancellationToken>,
    conn: PgConn,
    tick_ms: u64,
}

impl ActivityView {
    pub fn new(kind: ActivityKind, conn: PgConn, tick_ms: u64) -> Self {
        let table = match kind {
            ActivityKind::Queries | ActivityKind::Sessions => {
                DataTable::new(vec!["pid", "user", "db", "state", "wait", "query"])
            }
            ActivityKind::Locks => DataTable::new(vec!["pid", "mode", "granted", "relation", "query"]),
        };
        Self { id: ViewId::next(), kind, table, rows: vec![], locks: vec![], poll_token: None, conn, tick_ms }
    }

    pub fn selected_pid(&self) -> Option<i32> {
        let i = self.table.selected_index()?;
        match self.kind {
            ActivityKind::Queries | ActivityKind::Sessions => self.rows.get(i).map(|r| r.pid),
            ActivityKind::Locks => self.locks.get(i).map(|r| r.pid),
        }
    }

    pub fn conn(&self) -> &PgConn { &self.conn }
}

impl View for ActivityView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str {
        match self.kind {
            ActivityKind::Queries => "queries",
            ActivityKind::Locks => "locks",
            ActivityKind::Sessions => "sessions",
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
            // Build a confirm modal that fires pg_cancel_backend.
            if let Some(pid) = self.selected_pid() {
                let conn = self.conn.clone();
                let tx = ctx.event_tx.clone();
                let confirm = crate::views::confirm::ConfirmView::new(
                    "cancel backend",
                    format!("cancel pid {pid}?"),
                    format!("SELECT pg_cancel_backend({pid});"),
                    move || {
                        let conn = conn.clone();
                        let tx = tx.clone();
                        async move {
                            let r = cancel_backend(&conn, pid).await;
                            let toast = match &r {
                                Ok(true) => format!("cancelled pid {pid}"),
                                Ok(false) => format!("pid {pid} not cancelled (no such backend?)"),
                                Err(e) => format!("cancel failed: {e}"),
                            };
                            let _ = tx.send(AppEvent::Toast(toast)).await;
                            r.map(|b| if b { "cancelled".into() } else { "no-op".into() })
                        }
                    },
                );
                return Outcome::Push(Box::new(confirm));
            }
        }
        Outcome::Pass
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let token = CancellationToken::new();
        self.poll_token = Some(token.clone());

        let view_id = self.id;
        let conn = self.conn.clone();
        let kind = self.kind;
        let tx = ctx.event_tx.clone();
        let cadence = Duration::from_millis(self.tick_ms);

        tokio::spawn(async move {
            let mut tick = interval(cadence);
            loop {
                select! {
                    _ = token.cancelled() => break,
                    _ = tick.tick() => {
                        match kind {
                            ActivityKind::Queries => {
                                let r = activity(&conn, ActivityFilter::ActiveOnly).await;
                                let _ = tx.send(AppEvent::ViewData {
                                    view_id, payload: ViewPayload::Activity(r),
                                }).await;
                            }
                            ActivityKind::Sessions => {
                                let r = activity(&conn, ActivityFilter::All).await;
                                let _ = tx.send(AppEvent::ViewData {
                                    view_id, payload: ViewPayload::Activity(r),
                                }).await;
                            }
                            ActivityKind::Locks => {
                                let r = locks(&conn).await;
                                let _ = tx.send(AppEvent::ViewData {
                                    view_id, payload: ViewPayload::Locks(r),
                                }).await;
                            }
                        }
                    }
                }
            }
        });
    }

    fn on_leave(&mut self, _ctx: &mut Ctx) {
        if let Some(t) = self.poll_token.take() { t.cancel(); }
    }

    fn apply(&mut self, payload: ViewPayload) {
        match (self.kind, payload) {
            (ActivityKind::Queries | ActivityKind::Sessions, ViewPayload::Activity(Ok(rows))) => {
                self.rows = rows;
                let display: Vec<Vec<String>> = self.rows.iter().map(|r| vec![
                    r.pid.to_string(),
                    r.usename.clone().unwrap_or_default(),
                    r.datname.clone().unwrap_or_default(),
                    r.state.clone().unwrap_or_default(),
                    r.wait_event.clone().unwrap_or_default(),
                    r.query.clone().unwrap_or_default().chars().take(60).collect(),
                ]).collect();
                self.table.set_rows(display);
            }
            (ActivityKind::Locks, ViewPayload::Locks(Ok(rows))) => {
                self.locks = rows;
                let display: Vec<Vec<String>> = self.locks.iter().map(|r| vec![
                    r.pid.to_string(),
                    r.mode.clone().unwrap_or_default(),
                    if r.granted { "yes" } else { "no" }.into(),
                    r.relation.clone().unwrap_or_default(),
                    r.query.clone().unwrap_or_default().chars().take(60).collect(),
                ]).collect();
                self.table.set_rows(display);
            }
            _ => {}
        }
    }

    fn set_filter(&mut self, filter: &str) { self.table.set_filter(filter); }
    fn supports_filter(&self) -> bool { true }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
