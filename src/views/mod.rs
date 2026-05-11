//! View trait + dispatch types.

pub mod activity;
pub mod confirm;
pub mod connections;
pub mod databases;
pub mod help;
pub mod query;
pub mod row_detail;
pub mod rows;
pub mod schemas;
pub mod table_inspector;
pub mod tables;
pub mod themes;

use std::sync::atomic::{AtomicU64, Ordering};

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use tokio::sync::mpsc;

use crate::ui::theme::Theme;

/// Unique per-view id, used to drop stale async results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(pub u64);

static NEXT_VIEW_ID: AtomicU64 = AtomicU64::new(1);

impl ViewId {
    pub fn next() -> Self {
        ViewId(NEXT_VIEW_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Messages flowing into the main loop from spawned tasks.
pub enum AppEvent {
    /// Generic carrier for view-specific payloads. Carries view_id so stale
    /// results can be dropped.
    ViewData {
        view_id: ViewId,
        payload: ViewPayload,
    },
    /// Set a transient toast message in the footer.
    Toast(String),
    /// Replace the active connection with a newly connected one.
    ConnectionSwitched(crate::db::PgConn),
    /// Push a new view onto the stack from a spawned task.
    PushView(Box<dyn View>),
    /// Live-preview the named theme without persisting.
    PreviewTheme(&'static crate::ui::theme::Theme),
    /// Persist the current theme to the config file.
    PersistTheme(String),
    /// Restore a previously saved theme (used by ThemesView on Esc).
    RestoreTheme(&'static crate::ui::theme::Theme),
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppEvent::ViewData { view_id, payload } => f.debug_struct("ViewData")
                .field("view_id", view_id)
                .field("payload", payload)
                .finish(),
            AppEvent::Toast(s) => f.debug_tuple("Toast").field(s).finish(),
            AppEvent::ConnectionSwitched(c) => f.debug_tuple("ConnectionSwitched").field(c).finish(),
            AppEvent::PushView(_) => f.debug_tuple("PushView").field(&"<view>").finish(),
            AppEvent::PreviewTheme(t) => f.debug_tuple("PreviewTheme").field(&t.name).finish(),
            AppEvent::PersistTheme(s) => f.debug_tuple("PersistTheme").field(s).finish(),
            AppEvent::RestoreTheme(t) => f.debug_tuple("RestoreTheme").field(&t.name).finish(),
        }
    }
}

#[derive(Debug)]
pub enum ViewPayload {
    Databases(Result<Vec<crate::db::catalog::DatabaseInfo>, crate::error::DbError>),
    Schemas(Result<Vec<crate::db::catalog::SchemaInfo>, crate::error::DbError>),
    Tables(Result<Vec<crate::db::catalog::TableInfo>, crate::error::DbError>),
    Columns(Result<Vec<crate::db::catalog::ColumnInfo>, crate::error::DbError>),
    Indexes(Result<Vec<crate::db::catalog::IndexInfo>, crate::error::DbError>),
    Constraints(Result<Vec<crate::db::catalog::ConstraintInfo>, crate::error::DbError>),
    Size(Result<crate::db::catalog::TableSize, crate::error::DbError>),
    Rows(Result<crate::db::rows::Page, crate::error::DbError>),
    Query(Result<Vec<crate::db::query::ResultSet>, crate::error::DbError>),
    Activity(Result<Vec<crate::db::activity::ActivityRow>, crate::error::DbError>),
    Locks(Result<Vec<crate::db::activity::LockRow>, crate::error::DbError>),
    OpResult(Result<String, crate::error::DbError>),
}

/// Context passed to View methods so they can spawn tasks and emit events.
pub struct Ctx {
    pub event_tx: mpsc::Sender<AppEvent>,
}

impl Ctx {
    pub fn new(event_tx: mpsc::Sender<AppEvent>) -> Self {
        Self { event_tx }
    }
}

/// What a view returned to the dispatcher after handling a key.
pub enum Outcome {
    /// Key was handled, no further action.
    Consumed,
    /// Key wasn't handled by the view; let the dispatcher / palette have it.
    Pass,
    /// Push a new view onto the stack.
    Push(Box<dyn View>),
    /// Pop the current view.
    Pop,
    /// Quit the app.
    Quit,
}

pub trait View: Send {
    fn id(&self) -> ViewId;
    fn title(&self) -> &str;
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome;
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    fn on_enter(&mut self, _ctx: &mut Ctx) {}
    fn on_leave(&mut self, _ctx: &mut Ctx) {}
    /// Apply a typed payload addressed to this view. Default ignores.
    fn apply(&mut self, _payload: ViewPayload) {}

    /// Apply a substring filter to the view's primary table (if any).
    /// Default is a no-op for views without a filterable list.
    fn set_filter(&mut self, _filter: &str) {}

    /// Whether `/` should open filter mode for this view. Defaults to `false`
    /// so views with no list (e.g. SQL editor) can receive `/` as a literal key.
    fn supports_filter(&self) -> bool { false }

    /// Optional cast for the App to access view-specific state (e.g., selection).
    /// Implementations should return `Some(self)`.
    fn as_any(&self) -> Option<&dyn std::any::Any> { None }
}

/// A transient overlay rendered on top of the active view. Unlike `View`,
/// modals do not participate in navigation: they have no id, no title, and
/// cannot push/pop. They live in `App::modal: Option<Box<dyn Modal>>` and
/// are routed before the view stack.
pub trait Modal: Send {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> ModalOutcome;
    fn apply(&mut self, _payload: ViewPayload) {}
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    /// Footer hint string shown while this modal is open.
    fn hints(&self) -> &str { "" }
}

pub enum ModalOutcome {
    /// Key handled, keep the modal open.
    Consumed,
    /// Close the modal.
    Close,
}
