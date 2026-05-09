//! :query — multi-line editor + result pane.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Tabs},
};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{PgConn, query::{ResultSet, execute}},
    ui::{editor::{Editor, shell_out_to_editor}, table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct QueryView {
    id: ViewId,
    editor: Editor,
    results: Vec<ResultSet>,
    active_result: usize,
    active_table: DataTable,
    error: Option<String>,
    running: Option<CancellationToken>,
    conn: PgConn,
}

impl QueryView {
    pub fn new(conn: PgConn) -> Self {
        Self {
            id: ViewId::next(),
            editor: Editor::new(),
            results: vec![],
            active_result: 0,
            active_table: DataTable::new(vec![]),
            error: None,
            running: None,
            conn,
        }
    }

    fn run(&mut self, ctx: &mut Ctx) {
        if self.running.is_some() {
            // Already running — Ctrl-R re-issued. No-op for now.
            return;
        }
        let sql = self.editor.text();
        if sql.trim().is_empty() { return; }

        let view_id = self.id;
        let conn = self.conn.clone();
        let token = CancellationToken::new();
        self.running = Some(token.clone());
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = execute(&conn, &sql, token).await;
            let _ = tx.send(AppEvent::ViewData {
                view_id,
                payload: ViewPayload::Query(result),
            }).await;
        });
    }

    fn cancel(&mut self) {
        if let Some(t) = self.running.take() {
            t.cancel();
        }
    }

    fn select_result(&mut self, idx: usize) {
        if idx >= self.results.len() { return; }
        self.active_result = idx;
        let r = &self.results[idx];
        self.active_table = DataTable::new(r.headers.iter().map(String::as_str).collect());
        self.active_table.set_rows(r.rows.clone());
    }

    fn open_in_editor(&mut self) {
        let initial = self.editor.text();
        match shell_out_to_editor(&initial) {
            Ok(new) => self.editor.set_text(&new),
            Err(e) => self.error = Some(format!("editor: {e}")),
        }
    }
}

impl View for QueryView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "query" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(area);
        self.editor.render(f, chunks[0], theme);

        let titles: Vec<String> = self.results.iter().enumerate().map(|(i, r)| {
            if let Some(n) = r.affected {
                format!("#{} affected={n}", i + 1)
            } else {
                format!("#{} rows={}", i + 1, r.rows.len())
            }
        }).collect();
        let tabs = Tabs::new(titles)
            .select(self.active_result)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .style(Style::default().fg(theme.muted))
            .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
        f.render_widget(tabs, chunks[1]);

        if let Some(e) = &self.error {
            let p = Paragraph::new(format!("error: {e}"))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.error)))
                .style(Style::default().fg(theme.error));
            f.render_widget(p, chunks[2]);
        } else if self.results.is_empty() {
            let hint = if self.running.is_some() {
                "running… (Ctrl-C to cancel)"
            } else {
                "type SQL above; ^R or F5 to run; ^E to open in $EDITOR"
            };
            let p = Paragraph::new(hint)
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
                .style(Style::default().fg(theme.muted));
            f.render_widget(p, chunks[2]);
        } else {
            self.active_table.render(f, chunks[2], theme);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        // Run / cancel / open-in-editor / cycle-results bindings come first.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('r') => { self.run(ctx); return Outcome::Consumed; }
                KeyCode::Char('e') => { self.open_in_editor(); return Outcome::Consumed; }
                KeyCode::Char('c') => { self.cancel(); return Outcome::Consumed; }
                KeyCode::Char('n') => {
                    if !self.results.is_empty() {
                        self.select_result((self.active_result + 1) % self.results.len());
                    }
                    return Outcome::Consumed;
                }
                KeyCode::Char('p') => {
                    if !self.results.is_empty() {
                        let i = if self.active_result == 0 { self.results.len() - 1 } else { self.active_result - 1 };
                        self.select_result(i);
                    }
                    return Outcome::Consumed;
                }
                _ => {}
            }
        }
        if key.code == KeyCode::F(5) {
            self.run(ctx);
            return Outcome::Consumed;
        }
        // Forward everything else to the textarea.
        self.editor.area.input(tui_textarea::Input::from(key));
        Outcome::Consumed
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Query(res) = payload {
            self.running = None;
            match res {
                Ok(rs) => {
                    self.error = None;
                    self.results = rs;
                    self.active_result = 0;
                    if !self.results.is_empty() {
                        self.select_result(0);
                    }
                }
                Err(e) => {
                    self.error = Some(format!("{e}"));
                    self.results.clear();
                    self.active_table = DataTable::new(vec![]);
                }
            }
        }
    }

    fn set_filter(&mut self, filter: &str) { self.active_table.set_filter(filter); }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
