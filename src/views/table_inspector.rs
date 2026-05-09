//! Table inspector: tabs over rows | columns | indexes | constraints | size.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Tabs},
};

use crate::{
    db::{
        PgConn,
        catalog::{
            TableSize,
            list_columns, list_constraints, list_indexes, table_size,
        },
    },
    keys::{Motion, vim_motion},
    ui::{table::DataTable, theme::Theme},
    views::{
        AppEvent, Ctx, Outcome, View, ViewId, ViewPayload,
        rows::RowsView,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab { Rows, Columns, Indexes, Constraints, Size }
const TABS: &[Tab] = &[Tab::Rows, Tab::Columns, Tab::Indexes, Tab::Constraints, Tab::Size];

fn tab_label(t: Tab) -> &'static str {
    match t {
        Tab::Rows => "rows",
        Tab::Columns => "columns",
        Tab::Indexes => "indexes",
        Tab::Constraints => "constraints",
        Tab::Size => "size",
    }
}

pub struct TableInspectorView {
    id: ViewId,
    conn: PgConn,
    schema: String,
    name: String,
    active: Tab,
    rows: RowsView,
    columns: DataTable,
    indexes: DataTable,
    constraints: DataTable,
    size: Option<TableSize>,
    error: Option<String>,
}

impl TableInspectorView {
    pub fn new(conn: PgConn, schema: String, name: String) -> Self {
        Self {
            id: ViewId::next(),
            rows: RowsView::new(conn.clone(), schema.clone(), name.clone()),
            conn,
            schema,
            name,
            active: Tab::Rows,
            columns: DataTable::new(vec!["name", "type", "nullable", "default", "comment"]),
            indexes: DataTable::new(vec!["name", "definition", "size", "scans"]),
            constraints: DataTable::new(vec!["name", "kind", "definition"]),
            size: None,
            error: None,
        }
    }

    fn fetch_active(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let schema = self.schema.clone();
        let name = self.name.clone();
        let tx = ctx.event_tx.clone();
        match self.active {
            Tab::Rows => self.rows.on_enter(ctx),
            Tab::Columns => {
                tokio::spawn(async move {
                    let r = list_columns(&conn, &schema, &name).await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::Columns(r),
                    }).await;
                });
            }
            Tab::Indexes => {
                tokio::spawn(async move {
                    let r = list_indexes(&conn, &schema, &name).await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::Indexes(r),
                    }).await;
                });
            }
            Tab::Constraints => {
                tokio::spawn(async move {
                    let r = list_constraints(&conn, &schema, &name).await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::Constraints(r),
                    }).await;
                });
            }
            Tab::Size => {
                tokio::spawn(async move {
                    let r = table_size(&conn, &schema, &name).await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::Size(r),
                    }).await;
                });
            }
        }
    }

    fn cycle_tab(&mut self, dir: i32, ctx: &mut Ctx) {
        let cur = TABS.iter().position(|t| *t == self.active).unwrap_or(0) as i32;
        let len = TABS.len() as i32;
        let next = ((cur + dir).rem_euclid(len)) as usize;
        self.active = TABS[next];
        self.fetch_active(ctx);
    }

    pub fn rows_view(&self) -> &RowsView { &self.rows }
    pub fn schema(&self) -> &str { &self.schema }
    pub fn name(&self) -> &str { &self.name }
}

impl View for TableInspectorView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "table" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let titles: Vec<String> = TABS.iter().map(|t| tab_label(*t).to_string()).collect();
        let active_idx = TABS.iter().position(|t| *t == self.active).unwrap_or(0);
        let tabs = Tabs::new(titles)
            .select(active_idx)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .style(Style::default().fg(theme.muted))
            .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
        f.render_widget(tabs, chunks[0]);

        match self.active {
            Tab::Rows => self.rows.render(f, chunks[1], theme),
            Tab::Columns => self.columns.render(f, chunks[1], theme),
            Tab::Indexes => self.indexes.render(f, chunks[1], theme),
            Tab::Constraints => self.constraints.render(f, chunks[1], theme),
            Tab::Size => render_size(f, chunks[1], theme, self.size.as_ref(), self.error.as_deref()),
        }
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            match m {
                Motion::Left => { self.cycle_tab(-1, ctx); return Outcome::Consumed; }
                Motion::Right => { self.cycle_tab(1, ctx); return Outcome::Consumed; }
                _ => {}
            }
        }
        if self.active == Tab::Rows && key.code == KeyCode::Char('a') {
            return Outcome::Pass;
        }
        match self.active {
            Tab::Rows => self.rows.handle_key(key, ctx),
            Tab::Columns => motion_only(&mut self.columns, key),
            Tab::Indexes => motion_only(&mut self.indexes, key),
            Tab::Constraints => motion_only(&mut self.constraints, key),
            Tab::Size => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        self.fetch_active(ctx);
    }

    fn apply(&mut self, payload: ViewPayload) {
        match payload {
            ViewPayload::Rows(_) => self.rows.apply(payload),
            ViewPayload::Columns(Ok(cols)) => {
                let display: Vec<Vec<String>> = cols.iter().map(|c| vec![
                    c.name.clone(),
                    c.data_type.clone(),
                    if c.nullable { "yes".into() } else { "no".into() },
                    c.default.clone().unwrap_or_default(),
                    c.comment.clone().unwrap_or_default(),
                ]).collect();
                self.columns.set_rows(display);
                self.error = None;
            }
            ViewPayload::Columns(Err(e)) => self.error = Some(e.to_string()),
            ViewPayload::Indexes(Ok(ix)) => {
                let display: Vec<Vec<String>> = ix.iter().map(|i| vec![
                    i.name.clone(),
                    i.definition.clone(),
                    human_bytes(i.size_bytes),
                    i.scans.to_string(),
                ]).collect();
                self.indexes.set_rows(display);
                self.error = None;
            }
            ViewPayload::Indexes(Err(e)) => self.error = Some(e.to_string()),
            ViewPayload::Constraints(Ok(con)) => {
                let display: Vec<Vec<String>> = con.iter().map(|c| vec![
                    c.name.clone(),
                    c.kind.clone(),
                    c.definition.clone(),
                ]).collect();
                self.constraints.set_rows(display);
                self.error = None;
            }
            ViewPayload::Constraints(Err(e)) => self.error = Some(e.to_string()),
            ViewPayload::Size(Ok(s)) => { self.size = Some(s); self.error = None; }
            ViewPayload::Size(Err(e)) => self.error = Some(e.to_string()),
            _ => {}
        }
    }

    fn set_filter(&mut self, filter: &str) {
        match self.active {
            Tab::Rows => self.rows.set_filter(filter),
            Tab::Columns => self.columns.set_filter(filter),
            Tab::Indexes => self.indexes.set_filter(filter),
            Tab::Constraints => self.constraints.set_filter(filter),
            Tab::Size => {} // no table on the Size tab
        }
    }
    fn supports_filter(&self) -> bool { true }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}

fn motion_only(t: &mut DataTable, key: KeyEvent) -> Outcome {
    if let Some(m) = vim_motion(key) {
        t.move_motion(m);
        return Outcome::Consumed;
    }
    Outcome::Pass
}

fn render_size(f: &mut Frame, area: Rect, theme: &Theme, size: Option<&TableSize>, error: Option<&str>) {
    use ratatui::widgets::Paragraph;
    let body = if let Some(e) = error {
        format!("error: {e}")
    } else if let Some(s) = size {
        format!(
            "total:    {}\nheap:     {}\nindexes:  {}\ntoast:    {}\nrows (estimated): {}",
            human_bytes(s.total_bytes),
            human_bytes(s.heap_bytes),
            human_bytes(s.indexes_bytes),
            human_bytes(s.toast_bytes),
            s.estimated_rows,
        )
    } else {
        "loading…".to_string()
    };
    let p = Paragraph::new(body)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
        .style(Style::default().fg(theme.fg));
    f.render_widget(p, area);
}

fn human_bytes(bytes: i64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K { return format!("{bytes} B"); }
    if b < K * K { return format!("{:.1} KB", b / K); }
    if b < K * K * K { return format!("{:.1} MB", b / K / K); }
    format!("{:.2} GB", b / K / K / K)
}
