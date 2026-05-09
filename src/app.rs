//! App struct, view stack, main event loop.

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::Frame;
use tokio::{select, sync::mpsc, time::interval};

use crate::{
    db::PgConn,
    error::Result,
    term::Tui,
    ui::{self, footer, header, palette::{self, Cmd, Palette}, theme::{self, Theme}},
    views::{AppEvent, Ctx, Outcome, View},
};

const EVENT_CHANNEL_BUFFER: usize = 64;
const TICK_MS: u64 = 100;

pub struct App {
    pub views: Vec<Box<dyn View>>,
    pub palette: Palette,
    pub theme: &'static Theme,
    pub toast: Option<String>,
    pub event_tx: mpsc::Sender<AppEvent>,
    pub event_rx: mpsc::Receiver<AppEvent>,
    pub should_quit: bool,
    pub conn: Option<PgConn>,
    pub current_schema: String,
    pub config: crate::config::Config,
    pub config_path: std::path::PathBuf,
    /// When `Some`, the user is typing into the live filter buffer; the buffer
    /// content is mirrored to the active view via `set_filter` on every change.
    pub filter_input: Option<String>,
}

impl App {
    pub fn new(config: crate::config::Config, config_path: std::path::PathBuf) -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        let theme = theme::by_name(&config.ui.theme).unwrap_or(&theme::DEFAULT);
        Self {
            views: Vec::new(),
            palette: Palette::default(),
            theme,
            toast: None,
            event_tx,
            event_rx,
            should_quit: false,
            conn: None,
            current_schema: "public".to_string(),
            config,
            config_path,
            filter_input: None,
        }
    }

    pub fn push(&mut self, mut v: Box<dyn View>) {
        let mut ctx = Ctx::new(self.event_tx.clone());
        if let Some(top) = self.views.last_mut() {
            top.on_leave(&mut ctx);
        }
        v.on_enter(&mut ctx);
        self.views.push(v);
    }

    pub fn push_view(&mut self, v: Box<dyn View>) { self.push(v); }

    pub fn pop(&mut self) {
        let mut ctx = Ctx::new(self.event_tx.clone());
        if let Some(mut v) = self.views.pop() {
            v.on_leave(&mut ctx);
        }
        if let Some(top) = self.views.last_mut() {
            top.on_enter(&mut ctx);
        }
    }

    pub fn set_connection(&mut self, conn: PgConn) {
        let label = conn.label.clone();
        self.conn = Some(conn);
        self.toast = Some(format!("connected: {label}"));
    }

    fn apply_filter(&mut self, filter: &str) {
        if let Some(top) = self.views.last_mut() {
            top.set_filter(filter);
        }
    }

    fn handle_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Consumed | Outcome::Pass => {}
            Outcome::Push(v) => self.push(v),
            Outcome::Pop => self.pop(),
            Outcome::Quit => self.should_quit = true,
        }
    }

    fn dispatch_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Quit => self.should_quit = true,
            Cmd::Theme(name) => match theme::by_name(&name) {
                Some(t) => {
                    self.theme = t;
                    self.toast = Some(format!("theme: {}", t.name));
                }
                None => self.toast = Some(format!("unknown theme: {name}")),
            },
            Cmd::Open(verb) => self.open(&verb),
            Cmd::Connect(_) => self.toast = Some("connect not yet wired".into()),
            Cmd::Terminate(pid) => {
                let conn = match self.conn.clone() {
                    Some(c) => c,
                    None => { self.toast = Some("not connected".into()); return; }
                };
                let tx = self.event_tx.clone();
                let confirm = crate::views::confirm::ConfirmView::new(
                    "terminate backend",
                    format!("forcefully terminate pid {pid}? in-flight transaction will be aborted."),
                    format!("SELECT pg_terminate_backend({pid});"),
                    move || {
                        let conn = conn.clone();
                        let tx = tx.clone();
                        async move {
                            let r = crate::db::activity::terminate_backend(&conn, pid).await;
                            let toast = match &r {
                                Ok(true) => format!("terminated pid {pid}"),
                                Ok(false) => format!("pid {pid} not terminated (no such backend?)"),
                                Err(e) => format!("terminate failed: {e}"),
                            };
                            let _ = tx.send(AppEvent::Toast(toast)).await;
                            r.map(|b| if b { "terminated".into() } else { "no-op".into() })
                        }
                    },
                );
                self.push(Box::new(confirm));
            }
            Cmd::Unknown(s) => self.toast = Some(format!("unknown command: {s}")),
        }
    }

    fn open(&mut self, verb: &str) {
        // Verbs that work without an active connection:
        match verb {
            "connections" => {
                use crate::views::connections::ConnectionsView;
                let active = self.conn.as_ref().map(|c| c.label.clone());
                self.push(Box::new(ConnectionsView::new(&self.config, active.as_deref())));
                return;
            }
            "themes" => {
                use crate::views::themes::ThemesView;
                self.push(Box::new(ThemesView::new(self.theme)));
                return;
            }
            "help" => {
                self.push(Box::new(crate::views::help::HelpView::new()));
                return;
            }
            _ => {}
        }
        // Everything else needs a connection.
        use crate::views::{
            activity::{ActivityKind, ActivityView},
            databases::DatabasesView, query::QueryView, schemas::SchemasView, tables::TablesView,
        };
        let conn = match self.conn.clone() {
            Some(c) => c,
            None => {
                self.toast = Some("not connected — :connections to pick one".into());
                return;
            }
        };
        match verb {
            "databases" | "db" => self.push(Box::new(DatabasesView::new(conn))),
            "schemas" | "sc" => self.push(Box::new(SchemasView::new(conn))),
            "tables" | "tb" => self.push(Box::new(TablesView::new(conn, self.current_schema.clone()))),
            "query" | "sql" => self.push(Box::new(QueryView::new(conn))),
            "queries" => self.push(Box::new(ActivityView::new(ActivityKind::Queries, conn, self.config.ui.tick_ms))),
            "locks" => self.push(Box::new(ActivityView::new(ActivityKind::Locks, conn, self.config.ui.tick_ms))),
            "sessions" => self.push(Box::new(ActivityView::new(ActivityKind::Sessions, conn, self.config.ui.tick_ms))),
            other => self.toast = Some(format!("not yet wired: :{other}")),
        }
    }

    fn handle_enter_drilldown(&mut self) -> Outcome {
        use crate::views::{
            databases::DatabasesView, schemas::SchemasView, tables::TablesView,
        };
        // Snapshot the active view's title to decide what to do.
        let title = match self.views.last() {
            Some(v) => v.title().to_string(),
            None => return Outcome::Consumed,
        };
        // ConnectionsView Enter works without an active connection.
        if title == "connections" {
            use crate::views::connections::ConnectionsView;
            let top = self.views.last().unwrap();
            let view = top.as_any().and_then(|a| a.downcast_ref::<ConnectionsView>());
            if let Some(v) = view
                && let Some(name) = v.selected_name()
                && let Some(cfg) = self.config.find_connection(name)
            {
                let cfg_clone = cfg.clone();
                let name = name.to_string();
                let event_tx = self.event_tx.clone();
                tokio::spawn(async move {
                    match cfg_clone.resolve_secrets()
                        .and_then(|c| c.as_target())
                    {
                        Ok(target) => {
                            match crate::db::PgConn::connect(&target, name).await {
                                Ok(conn) => {
                                    let _ = event_tx.send(AppEvent::ConnectionSwitched(conn)).await;
                                }
                                Err(e) => {
                                    let _ = event_tx.send(AppEvent::Toast(format!("connect failed: {e}"))).await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(AppEvent::Toast(format!("config error: {e}"))).await;
                        }
                    }
                });
            }
            return Outcome::Consumed;
        }
        let Some(conn) = self.conn.clone() else {
            return Outcome::Consumed;
        };
        match title.as_str() {
            "databases" => {
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<DatabasesView>());
                if let Some(v) = view {
                    if let Some(d) = v.selected() {
                        let name = d.name.clone();
                        let old_conn = conn.clone();
                        let event_tx = self.event_tx.clone();
                        tokio::spawn(async move {
                            match switch_database(&old_conn, &name).await {
                                Ok(new_conn) => {
                                    let _ = event_tx.send(AppEvent::ConnectionSwitched(new_conn)).await;
                                }
                                Err(e) => {
                                    let _ = event_tx.send(AppEvent::Toast(format!("switch failed: {e}"))).await;
                                }
                            }
                        });
                    }
                }
                Outcome::Consumed
            }
            "schemas" => {
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<SchemasView>());
                if let Some(v) = view {
                    if let Some(s) = v.selected() {
                        self.current_schema = s.name.clone();
                        return Outcome::Push(Box::new(TablesView::new(conn, self.current_schema.clone())));
                    }
                }
                Outcome::Consumed
            }
            "tables" => {
                use crate::views::table_inspector::TableInspectorView;
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<crate::views::tables::TablesView>());
                if let Some(v) = view {
                    if let Some(t) = v.selected() {
                        return Outcome::Push(Box::new(TableInspectorView::new(
                            conn,
                            t.schema.clone(),
                            t.name.clone(),
                        )));
                    }
                }
                Outcome::Consumed
            }
            "table" => {
                use crate::db::catalog::primary_key;
                use crate::ui::detail::DetailView;
                use crate::views::row_detail::RowDetailView;
                use crate::views::table_inspector::TableInspectorView;
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<TableInspectorView>());
                if let Some(insp) = view {
                    let conn = self.conn.clone().expect("conn present");
                    let schema = insp.schema().to_string();
                    let table = insp.name().to_string();
                    let event_tx = self.event_tx.clone();
                    // Cache the rows-view selection BEFORE we move into async (which we won't —
                    // we do the synchronous slice now and dispatch the async fetch for PK).
                    // Pull PK in a spawned task; then push the detail view via an internal event.
                    if let Some(fields_template) = insp.rows_view().detail_fields(&[]) {
                        let conn_for_pk = conn.clone();
                        tokio::spawn(async move {
                            let pk = primary_key(&conn_for_pk, &schema, &table).await.unwrap_or_default();
                            let pk_names: Vec<String> = pk.iter().map(|c| c.name.clone()).collect();
                            let mut fields = fields_template;
                            for f in &mut fields {
                                f.is_pk = pk_names.iter().any(|n| n == &f.name);
                            }
                            let detail = DetailView::new(fields);
                            let view = RowDetailView::new(conn, schema, table, pk, detail);
                            let _ = event_tx.send(AppEvent::PushView(Box::new(view))).await;
                        });
                    }
                }
                Outcome::Consumed
            }
            _ => Outcome::Consumed,
        }
    }

    fn handle_insert_request(&mut self) -> Outcome {
        use crate::db::catalog::primary_key;
        use crate::ui::detail::DetailView;
        use crate::views::row_detail::RowDetailView;
        use crate::views::table_inspector::TableInspectorView;
        let top = match self.views.last() {
            Some(v) if v.title() == "table" => v,
            _ => return Outcome::Pass,
        };
        let view = top.as_any().and_then(|a| a.downcast_ref::<TableInspectorView>());
        if let Some(insp) = view {
            let conn = self.conn.clone().expect("conn present");
            let schema = insp.schema().to_string();
            let table = insp.name().to_string();
            let event_tx = self.event_tx.clone();
            if let Some(fields_template) = insp.rows_view().blank_fields(&[]) {
                let conn_for_pk = conn.clone();
                tokio::spawn(async move {
                    let pk = primary_key(&conn_for_pk, &schema, &table).await.unwrap_or_default();
                    let pk_names: Vec<String> = pk.iter().map(|c| c.name.clone()).collect();
                    let mut fields = fields_template;
                    for f in &mut fields {
                        f.is_pk = pk_names.iter().any(|n| n == &f.name);
                    }
                    let mut detail = DetailView::new(fields);
                    detail.enter_edit();
                    let mut view = RowDetailView::new(conn, schema, table, pk, detail);
                    view.set_insert_mode();
                    let _ = event_tx.send(AppEvent::PushView(Box::new(view))).await;
                });
            }
        }
        Outcome::Consumed
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // global: Ctrl-Q quits unconditionally
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return;
        }

        // palette mode owns the keys until closed
        if self.palette.open {
            match key.code {
                KeyCode::Esc => self.palette.close(),
                KeyCode::Enter => {
                    let cmd = palette::parse(&self.palette.buffer);
                    self.palette.close();
                    self.toast = None;
                    self.dispatch_cmd(cmd);
                }
                KeyCode::Tab => self.palette.accept_suggestion(),
                KeyCode::Backspace => self.palette.backspace(),
                KeyCode::Char(c) => self.palette.push(c),
                _ => {}
            }
            return;
        }

        // open palette
        if key.code == KeyCode::Char(':') {
            self.palette.open();
            return;
        }

        // filter mode owns the keys until closed (Esc clears, Enter accepts).
        if let Some(buf) = self.filter_input.as_mut() {
            match key.code {
                KeyCode::Esc => {
                    self.filter_input = None;
                    self.apply_filter("");
                }
                KeyCode::Enter => {
                    self.filter_input = None;
                }
                KeyCode::Backspace => {
                    buf.pop();
                    let snapshot = buf.clone();
                    self.apply_filter(&snapshot);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    let snapshot = buf.clone();
                    self.apply_filter(&snapshot);
                }
                _ => {}
            }
            return;
        }

        // open filter mode
        if key.code == KeyCode::Char('/')
            && self.views.last().is_some_and(|v| v.supports_filter())
        {
            self.filter_input = Some(String::new());
            self.apply_filter("");
            return;
        }

        // open help modal
        if key.code == KeyCode::Char('?') {
            self.push(Box::new(crate::views::help::HelpView::new()));
            return;
        }

        // forward to active view
        if let Some(top) = self.views.last_mut() {
            let mut ctx = Ctx::new(self.event_tx.clone());
            let outcome = top.handle_key(key, &mut ctx);
            // Esc bubbles to Pop if the view passed; Enter triggers drilldown
            let outcome = match outcome {
                Outcome::Pass if key.code == KeyCode::Esc => Outcome::Pop,
                Outcome::Pass if key.code == KeyCode::Enter => self.handle_enter_drilldown(),
                Outcome::Pass if key.code == KeyCode::Char('a') => self.handle_insert_request(),
                other => other,
            };
            self.handle_outcome(outcome);
            if self.views.is_empty() && key.code == KeyCode::Esc {
                self.should_quit = true;
            }
        } else if key.code == KeyCode::Esc {
            self.should_quit = true;
        }
    }

    fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Toast(s) => self.toast = Some(s),
            AppEvent::ViewData { view_id, payload } => {
                if let Some(top) = self.views.last_mut() {
                    if top.id() == view_id {
                        top.apply(payload);
                    }
                }
            }
            AppEvent::ConnectionSwitched(new_conn) => {
                let label = new_conn.label.clone();
                self.conn = Some(new_conn);
                self.toast = Some(format!("switched to {label}"));

                // If the picker is on top and there's a view underneath, pop it so the
                // user returns to whatever they were doing (now reconnected). If it's
                // the only view in the stack (landing page), leave it — popping would
                // empty the stack and quit the app.
                let pop_picker = self.views.len() > 1
                    && self.views.last()
                        .and_then(|v| v.as_any())
                        .and_then(|a| a.downcast_ref::<crate::views::connections::ConnectionsView>())
                        .is_some();
                if pop_picker {
                    self.pop(); // also calls on_enter on the new top
                } else {
                    let mut ctx = Ctx::new(self.event_tx.clone());
                    if let Some(top) = self.views.last_mut() {
                        top.on_enter(&mut ctx);
                    }
                }
            }
            AppEvent::PushView(v) => self.push(v),
            AppEvent::PreviewTheme(t) => {
                self.theme = t;
            }
            AppEvent::RestoreTheme(t) => {
                self.theme = t;
            }
            AppEvent::PersistTheme(name) => {
                match theme::by_name(&name) {
                    Some(t) => {
                        self.theme = t;
                        self.config.ui.theme = name;
                        if let Err(e) = self.config.save(&self.config_path) {
                            self.toast = Some(format!("save failed: {e}"));
                        } else {
                            self.toast = Some(format!("theme: {} (saved)", t.name));
                        }
                    }
                    None => self.toast = Some(format!("unknown theme: {name}")),
                }
            }
        }
    }

    fn render(&mut self, f: &mut Frame) {
        let [head, main, foot] = ui::split(f.area());

        let title_owned = match &self.conn {
            Some(c) => format!("postui  ·  {}", c.label),
            None => "postui  ·  not connected".to_string(),
        };
        let breadcrumb = self.views.last().map(|v| v.title()).unwrap_or("");
        header::render(f, head, self.theme, &title_owned, breadcrumb);

        if let Some(top) = self.views.last_mut() {
            top.render(f, main, self.theme);
        } else {
            ui::render_main_placeholder(f, main);
        }

        let hints = match self.views.last() {
            Some(_) => "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help",
            None => "[:] palette  [^Q] quit",
        };
        let toast_or_filter: String;
        let footer_text: Option<&str> = if let Some(buf) = &self.filter_input {
            toast_or_filter = format!("/{buf}");
            Some(toast_or_filter.as_str())
        } else {
            self.toast.as_deref()
        };
        footer::render(f, foot, self.theme, hints, footer_text, &self.palette);
    }

    pub async fn run(mut self, terminal: &mut Tui) -> Result<()> {
        let mut events = EventStream::new();
        let mut tick = interval(Duration::from_millis(TICK_MS));

        loop {
            terminal.draw(|f| self.render(f))?;
            if self.should_quit {
                return Ok(());
            }

            select! {
                maybe_term = events.next() => {
                    match maybe_term {
                        Some(Ok(Event::Key(k))) if k.kind == crossterm::event::KeyEventKind::Press => {
                            self.handle_key(k);
                            while let Ok(ev) = self.event_rx.try_recv() {
                                self.handle_event(ev);
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => tracing::error!(?e, "crossterm event error"),
                        None => return Ok(()),
                    }
                }
                Some(ev) = self.event_rx.recv() => {
                    self.handle_event(ev);
                }
                _ = tick.tick() => {
                    let mut ctx = Ctx::new(self.event_tx.clone());
                    if let Some(top) = self.views.last_mut() {
                        top.on_tick(&mut ctx);
                    }
                }
            }
        }
    }
}

async fn switch_database(old: &PgConn, new_db: &str) -> crate::error::Result<PgConn> {
    // Pull the current host / port / user from the live conn.
    let row = old.client()
        .query_one(
            "SELECT inet_server_port(), current_user",
            &[],
        )
        .await
        .map_err(|e| crate::error::DbError::Query {
            sql: "current params".into(),
            source: Box::new(e),
        })?;
    let port: i32 = row.get::<_, Option<i32>>(0).unwrap_or(5432);
    let user: String = row.get(1);
    // We can't recover the password from a live conn; the user must rely on
    // ~/.pgpass or a passwordless local auth for db-switch in v1.
    // Use Unix socket where available; fallback to localhost.
    let target = format!("host=/var/run/postgresql user={user} dbname={new_db} port={port} application_name=postui");
    let label = format!("{user}@{new_db}");
    Ok(PgConn::connect(&target, label).await?)
}
