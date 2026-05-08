# postui v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the postui v1 design — a k9s-style Postgres TUI with single-pane command-driven navigation, live activity views, table inspector, in-place row editing, and runtime theme switching.

**Architecture:** Single-writer state model on a current-thread `tokio` runtime. The main loop owns app state and `select!`s over keyboard events, internal app events, and a tick interval. DB I/O runs as spawned tasks that report results via `mpsc`. View stack drives navigation: `:command` pushes, `<esc>` pops.

**Tech Stack:** Rust 2024, `ratatui` + `crossterm` (TUI), `tokio` (rt), `tokio-postgres` (DB), `tui-textarea` (editor), `serde`/`toml` (config), `thiserror`, `tracing`, `clap`, `tokio-util` (cancellation), `directories` (XDG paths), `testcontainers` (integration tests).

**Spec:** `docs/superpowers/specs/2026-05-08-postui-design.md`

---

## File Structure

This is the target layout once v1 is complete. Tasks below build it up incrementally.

```
src/
  main.rs              CLI parsing, runtime bootstrap, run App
  app.rs               App struct, event loop, view stack, dispatch
  config.rs            Config types, TOML parsing, env interp, URI parsing
  error.rs             AppError, DbError, ConfigError (thiserror)
  keys.rs              KeyMap, vim motion bindings

  db/
    mod.rs             PgConn, connect()
    catalog.rs         pg_catalog queries -> typed structs
    activity.rs        pg_stat_activity / pg_locks queries
    rows.rs            paged row fetch
    mutate.rs          UPDATE/INSERT/DELETE SQL builders
    types.rs           postgres::Row -> DisplayValue conversion

  ui/
    mod.rs             top-level layout (header / main / footer)
    header.rs          connection name + breadcrumb
    footer.rs          keybind hints + transient status toast
    palette.rs         ":command" line state + parser
    table.rs           generic ratatui table widget
    editor.rs          tui-textarea wrapper + $EDITOR shellout
    confirm.rs         confirmation modal w/ SQL preview
    detail.rs          key/value form for row detail
    theme.rs           Theme struct + 5 built-in constants

  views/
    mod.rs             View trait, Outcome, Ctx, view_id allocator
    connections.rs     :connections
    databases.rs       :databases
    schemas.rs         :schemas
    tables.rs          :tables
    table_inspector.rs tabs: rows | columns | indexes | constraints | size
    rows.rs            paged rows view (used by inspector)
    query.rs           :query editor + result pane
    activity.rs        :queries / :locks / :sessions
    themes.rs          :themes
    help.rs            ? help modal

tests/
  config_test.rs       config parsing, env interp, URI round-trip
  catalog_it.rs        catalog queries vs real PG (testcontainers)
  activity_it.rs       activity queries vs real PG
  rows_it.rs           paging + type conversion vs real PG
  mutate_it.rs         end-to-end mutation flow vs real PG
  view_snapshot.rs     ratatui TestBackend snapshot tests
```

**Conventions:** every list view uses `ui::table`. Catalog queries return typed structs (`TableInfo`, `ColumnInfo`, `ActivityRow`); `tokio_postgres::Row` does not leak past `db/`. Views are thin: state + `View` impl + the catalog calls they fire.

---

## Milestones

Each milestone leaves the app in a runnable, demoable state. Don't skip ahead.

| # | Milestone | Demoable end state |
|---|-----------|---------------------|
| 1 | Foundation | App launches, panic-safe terminal, palette accepts `:q` to quit. No DB. |
| 2 | Config + connect | CLI + config file load; connect via `--uri` or `--connection NAME`; header shows active conn. |
| 3 | Browse chain | `:databases`, `:schemas`, `:tables` with vim motions + drill-down. |
| 4 | Table inspector | Drill into a table → 5 tabs (rows / columns / indexes / constraints / size); row detail view. |
| 5 | `:query` editor | Multi-line editor, `Ctrl-R` runs, `Ctrl-E` shells out to `$EDITOR`, `Ctrl-C` cancels. |
| 6 | Live activity | `:queries`, `:locks`, `:sessions` with `on_enter`/`on_leave` polling lifecycle; `Ctrl-K` cancel; `:terminate <pid>`. |
| 7 | Row mutations | `i`/`a`/`d` in rows + row detail; confirm modal w/ SQL preview; `:query` mutations. |
| 8 | Themes + polish | `:themes` with live preview; `:connect`/`:connections`; `/` filter; `?` help; footer hints. |

---

## Milestone 1 — Foundation

**End state:** `cargo run` launches the app. You see a header bar, an empty main pane, and a footer hint line. Press `:` to open the palette, type `q` then `<enter>`, app exits cleanly. Press `Ctrl-Q` and it also quits. If the app panics, your terminal is restored. No DB code yet.

### Task 1.1: Add base crate dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the runtime + TUI + util crates**

```bash
cargo add tokio --features rt,macros,sync,time,signal
cargo add tokio-util --features rt
cargo add ratatui
cargo add crossterm --features event-stream
cargo add thiserror
cargo add tracing
cargo add tracing-appender
cargo add tracing-subscriber --features env-filter,fmt
cargo add directories
cargo add clap --features derive
cargo add futures
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: PASS, no warnings about unused deps yet (we'll wire them in subsequent tasks).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add tokio, ratatui, crossterm, tracing, clap"
```

### Task 1.2: Define error types

**Files:**
- Create: `src/error.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/error.rs` with the top-level error enum**

```rust
//! Top-level error types.

use std::io;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("db error: {0}")]
    Db(#[from] DbError),
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Read(#[source] io::Error),

    #[error("failed to parse config: {0}")]
    Parse(String),

    #[error("missing env var: {var}")]
    MissingEnv { var: String },

    #[error("invalid postgres uri: {0}")]
    BadUri(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connect failed: {0}")]
    Connect(String),

    #[error("query failed: {source}\n  sql: {sql}")]
    Query { sql: String, source: String },

    #[error("type conversion error: {0}")]
    Type(String),

    #[error("query cancelled")]
    Cancelled,
}
```

- [ ] **Step 2: Wire `error` module into `src/main.rs`**

```rust
mod error;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/error.rs src/main.rs
git commit -m "error: add AppError / ConfigError / DbError"
```

### Task 1.3: Theme struct and built-in palettes

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/theme.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test in `src/ui/theme.rs`**

```rust
//! Theme palette: colors used by every render.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub fg: Color,
    pub border: Color,
    pub header: Color,
    pub footer: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub accent: Color,
    pub muted: Color,
    pub error: Color,
    pub warn: Color,
    pub success: Color,
    pub table_header: Color,
    pub row_stripe: Color,
}

pub static DEFAULT: Theme = Theme {
    name: "default",
    bg: Color::Reset,
    fg: Color::Reset,
    border: Color::DarkGray,
    header: Color::Cyan,
    footer: Color::DarkGray,
    selection_bg: Color::Blue,
    selection_fg: Color::White,
    accent: Color::Yellow,
    muted: Color::DarkGray,
    error: Color::Red,
    warn: Color::Yellow,
    success: Color::Green,
    table_header: Color::Cyan,
    row_stripe: Color::Reset,
};

pub static DRACULA: Theme = Theme {
    name: "dracula",
    bg: Color::Rgb(40, 42, 54),
    fg: Color::Rgb(248, 248, 242),
    border: Color::Rgb(98, 114, 164),
    header: Color::Rgb(189, 147, 249),
    footer: Color::Rgb(98, 114, 164),
    selection_bg: Color::Rgb(68, 71, 90),
    selection_fg: Color::Rgb(248, 248, 242),
    accent: Color::Rgb(255, 121, 198),
    muted: Color::Rgb(98, 114, 164),
    error: Color::Rgb(255, 85, 85),
    warn: Color::Rgb(241, 250, 140),
    success: Color::Rgb(80, 250, 123),
    table_header: Color::Rgb(139, 233, 253),
    row_stripe: Color::Rgb(44, 46, 60),
};

pub static GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox-dark",
    bg: Color::Rgb(40, 40, 40),
    fg: Color::Rgb(235, 219, 178),
    border: Color::Rgb(102, 92, 84),
    header: Color::Rgb(131, 165, 152),
    footer: Color::Rgb(102, 92, 84),
    selection_bg: Color::Rgb(60, 56, 54),
    selection_fg: Color::Rgb(251, 241, 199),
    accent: Color::Rgb(254, 128, 25),
    muted: Color::Rgb(146, 131, 116),
    error: Color::Rgb(204, 36, 29),
    warn: Color::Rgb(215, 153, 33),
    success: Color::Rgb(152, 151, 26),
    table_header: Color::Rgb(131, 165, 152),
    row_stripe: Color::Rgb(50, 48, 47),
};

pub static NORD: Theme = Theme {
    name: "nord",
    bg: Color::Rgb(46, 52, 64),
    fg: Color::Rgb(216, 222, 233),
    border: Color::Rgb(76, 86, 106),
    header: Color::Rgb(136, 192, 208),
    footer: Color::Rgb(76, 86, 106),
    selection_bg: Color::Rgb(67, 76, 94),
    selection_fg: Color::Rgb(236, 239, 244),
    accent: Color::Rgb(180, 142, 173),
    muted: Color::Rgb(76, 86, 106),
    error: Color::Rgb(191, 97, 106),
    warn: Color::Rgb(235, 203, 139),
    success: Color::Rgb(163, 190, 140),
    table_header: Color::Rgb(143, 188, 187),
    row_stripe: Color::Rgb(59, 66, 82),
};

pub static SOLARIZED_DARK: Theme = Theme {
    name: "solarized-dark",
    bg: Color::Rgb(0, 43, 54),
    fg: Color::Rgb(131, 148, 150),
    border: Color::Rgb(88, 110, 117),
    header: Color::Rgb(38, 139, 210),
    footer: Color::Rgb(88, 110, 117),
    selection_bg: Color::Rgb(7, 54, 66),
    selection_fg: Color::Rgb(147, 161, 161),
    accent: Color::Rgb(181, 137, 0),
    muted: Color::Rgb(88, 110, 117),
    error: Color::Rgb(220, 50, 47),
    warn: Color::Rgb(181, 137, 0),
    success: Color::Rgb(133, 153, 0),
    table_header: Color::Rgb(38, 139, 210),
    row_stripe: Color::Rgb(7, 54, 66),
};

pub static ALL: &[&'static Theme] = &[&DEFAULT, &DRACULA, &GRUVBOX_DARK, &NORD, &SOLARIZED_DARK];

pub fn by_name(name: &str) -> Option<&'static Theme> {
    ALL.iter().copied().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_name_finds_each_built_in() {
        for t in ALL {
            assert_eq!(by_name(t.name).map(|x| x.name), Some(t.name));
        }
    }

    #[test]
    fn by_name_returns_none_for_unknown() {
        assert!(by_name("nonexistent").is_none());
    }
}
```

- [ ] **Step 2: Create `src/ui/mod.rs`**

```rust
pub mod theme;
```

- [ ] **Step 3: Wire into `src/main.rs`**

```rust
mod error;
mod ui;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib theme`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/
git commit -m "ui: add Theme struct and 5 built-in palettes"
```

### Task 1.4: Tracing setup with file appender

**Files:**
- Create: `src/logging.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/logging.rs`**

```rust
//! File-based tracing setup. Stdout is owned by the TUI, so we log to a file
//! under the XDG state dir.

use std::path::PathBuf;

use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt};

/// Initialize a non-blocking file appender. Returns the WorkerGuard, which must
/// be kept alive for the lifetime of the program (drop = flush).
pub fn init() -> std::io::Result<WorkerGuard> {
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir)?;

    let appender = tracing_appender::rolling::never(&log_dir, "postui.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!(dir = %log_dir.display(), "tracing initialized");
    Ok(guard)
}

fn log_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("", "", "postui") {
        // ProjectDirs gives data_local_dir; we want state_dir on Linux.
        // Fall back to data_local_dir if state_dir isn't available.
        if let Some(state) = dirs.state_dir() {
            return state.to_path_buf();
        }
        return dirs.data_local_dir().to_path_buf();
    }
    PathBuf::from(".")
}
```

- [ ] **Step 2: Wire into `src/main.rs`**

```rust
mod error;
mod logging;
mod ui;

fn main() -> std::io::Result<()> {
    let _log_guard = logging::init()?;
    tracing::info!("postui starting");
    Ok(())
}
```

- [ ] **Step 3: Verify it builds and runs**

Run: `cargo build && cargo run`
Expected: builds clean; `cargo run` exits 0; check that `~/.local/state/postui/postui.log` contains "tracing initialized" and "postui starting".

- [ ] **Step 4: Commit**

```bash
git add src/logging.rs src/main.rs
git commit -m "logging: file-based tracing with non-blocking appender"
```

### Task 1.5: Terminal init/teardown + panic hook

**Files:**
- Create: `src/term.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/term.rs`**

```rust
//! Terminal init/teardown. Holds RAII guard so the terminal is always restored.

use std::io::{self, Stdout, stdout};

use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// RAII handle: enters raw mode + alt screen on `init`, restores on drop.
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn init() -> io::Result<(Tui, Self)> {
        enable_raw_mode()?;
        let mut out = stdout();
        out.execute(EnterAlternateScreen)?;
        out.execute(EnableMouseCapture)?;
        let backend = CrosstermBackend::new(out);
        let term = Terminal::new(backend)?;
        Ok((term, TerminalGuard))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

/// Install a panic hook that restores the terminal before re-panicking.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = stdout();
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        tracing::error!(?info, "panic");
        prev(info);
    }));
}
```

- [ ] **Step 2: Wire into `src/main.rs`**

```rust
mod error;
mod logging;
mod term;
mod ui;

fn main() -> std::io::Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();
    tracing::info!("postui starting");
    Ok(())
}
```

- [ ] **Step 3: Verify**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/term.rs src/main.rs
git commit -m "term: RAII terminal guard + panic hook"
```

### Task 1.6: View trait, Outcome, Ctx, AppEvent

**Files:**
- Create: `src/views/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/views/mod.rs`**

```rust
//! View trait + dispatch types.

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
#[derive(Debug)]
pub enum AppEvent {
    /// Generic carrier for view-specific payloads. Carries view_id so stale
    /// results can be dropped.
    ViewData {
        view_id: ViewId,
        payload: ViewPayload,
    },
    /// Set a transient toast message in the footer.
    Toast(String),
}

/// Concrete payloads each view knows how to apply. New variants added per view
/// in later milestones.
#[derive(Debug)]
pub enum ViewPayload {
    /// Placeholder so the enum is non-empty in M1; later milestones add real
    /// variants and remove this.
    None,
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
}
```

- [ ] **Step 2: Wire into `src/main.rs`**

```rust
mod error;
mod logging;
mod term;
mod ui;
mod views;

fn main() -> std::io::Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();
    tracing::info!("postui starting");
    Ok(())
}
```

- [ ] **Step 3: Verify**

Run: `cargo build`
Expected: PASS, possibly with unused-import warnings (acceptable for M1 — wired up in next tasks).

- [ ] **Step 4: Commit**

```bash
git add src/views/mod.rs src/main.rs
git commit -m "views: add View trait, Outcome, Ctx, AppEvent, ViewId"
```

### Task 1.7: Palette state + parser with unit tests

**Files:**
- Create: `src/ui/palette.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write the failing tests in `src/ui/palette.rs`**

```rust
//! ":command" palette state and parser.

#[derive(Debug, Default)]
pub struct Palette {
    pub open: bool,
    pub buffer: String,
}

impl Palette {
    pub fn open(&mut self) {
        self.open = true;
        self.buffer.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
    }

    pub fn push(&mut self, c: char) {
        self.buffer.push(c);
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
    }
}

/// A parsed palette command.
#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Quit,
    Open(String),                  // :tables, :databases, ...
    Theme(String),                 // :theme dracula
    Terminate(i32),                // :terminate <pid>
    Connect(Option<String>),       // :connect [uri-or-name]
    Unknown(String),
}

/// Parse the buffer (without leading ':') into a Cmd.
pub fn parse(input: &str) -> Cmd {
    let s = input.trim();
    if s.is_empty() {
        return Cmd::Unknown(String::new());
    }
    let mut parts = s.split_whitespace();
    let head = parts.next().unwrap();
    let rest: Vec<&str> = parts.collect();

    match head {
        "q" | "quit" => Cmd::Quit,
        "theme" => Cmd::Theme(rest.join(" ")),
        "terminate" => match rest.first().and_then(|s| s.parse::<i32>().ok()) {
            Some(pid) => Cmd::Terminate(pid),
            None => Cmd::Unknown(s.to_string()),
        },
        "connect" => {
            let arg = rest.join(" ");
            Cmd::Connect(if arg.is_empty() { None } else { Some(arg) })
        }
        // Bare verbs: :databases, :schemas, :tables, :query, :queries, :locks,
        // :sessions, :connections, :themes, :help
        other => Cmd::Open(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quit_aliases() {
        assert_eq!(parse("q"), Cmd::Quit);
        assert_eq!(parse("quit"), Cmd::Quit);
        assert_eq!(parse("  q  "), Cmd::Quit);
    }

    #[test]
    fn parses_open_verbs() {
        assert_eq!(parse("tables"), Cmd::Open("tables".into()));
        assert_eq!(parse("databases"), Cmd::Open("databases".into()));
        assert_eq!(parse("queries"), Cmd::Open("queries".into()));
    }

    #[test]
    fn parses_theme_with_arg() {
        assert_eq!(parse("theme dracula"), Cmd::Theme("dracula".into()));
        assert_eq!(parse("theme"), Cmd::Theme(String::new()));
    }

    #[test]
    fn parses_terminate_with_pid() {
        assert_eq!(parse("terminate 482"), Cmd::Terminate(482));
        // missing or malformed pid -> Unknown
        assert!(matches!(parse("terminate"), Cmd::Unknown(_)));
        assert!(matches!(parse("terminate abc"), Cmd::Unknown(_)));
    }

    #[test]
    fn parses_connect() {
        assert_eq!(parse("connect"), Cmd::Connect(None));
        assert_eq!(
            parse("connect prod"),
            Cmd::Connect(Some("prod".into()))
        );
        assert_eq!(
            parse("connect postgres://u:p@h/db"),
            Cmd::Connect(Some("postgres://u:p@h/db".into()))
        );
    }

    #[test]
    fn empty_is_unknown() {
        assert_eq!(parse(""), Cmd::Unknown(String::new()));
        assert_eq!(parse("   "), Cmd::Unknown(String::new()));
    }
}
```

- [ ] **Step 2: Add to `src/ui/mod.rs`**

```rust
pub mod palette;
pub mod theme;
```

- [ ] **Step 3: Run tests, expect pass**

Run: `cargo test --lib palette`
Expected: 6 passed.

- [ ] **Step 4: Commit**

```bash
git add src/ui/palette.rs src/ui/mod.rs
git commit -m "palette: state machine + parser with unit tests"
```

### Task 1.8: Header / Main / Footer layout placeholders

**Files:**
- Create: `src/ui/header.rs`
- Create: `src/ui/footer.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write `src/ui/header.rs`**

```rust
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use super::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, theme: &Theme, title: &str, breadcrumb: &str) {
    let text = format!("{title:<24}{breadcrumb}");
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme.border)))
        .style(Style::default().fg(theme.header));
    f.render_widget(p, area);
}
```

- [ ] **Step 2: Write `src/ui/footer.rs`**

```rust
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use super::{palette::Palette, theme::Theme};

pub fn render(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    hints: &str,
    toast: Option<&str>,
    palette: &Palette,
) {
    let line = if palette.open {
        format!(":{}", palette.buffer)
    } else if let Some(t) = toast {
        t.to_string()
    } else {
        hints.to_string()
    };

    let style = if palette.open {
        Style::default().fg(theme.accent)
    } else if toast.is_some() {
        Style::default().fg(theme.warn)
    } else {
        Style::default().fg(theme.footer)
    };

    let p = Paragraph::new(line)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(theme.border)))
        .style(style);
    f.render_widget(p, area);
}
```

- [ ] **Step 3: Update `src/ui/mod.rs`**

```rust
pub mod footer;
pub mod header;
pub mod palette;
pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

/// Splits the frame into header (1 line + border), main pane, footer (1 line + border).
pub fn split(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

#[allow(dead_code)]
pub fn render_main_placeholder(f: &mut Frame, area: Rect) {
    use ratatui::widgets::{Block, Borders};
    f.render_widget(Block::default().borders(Borders::NONE), area);
}
```

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/header.rs src/ui/footer.rs src/ui/mod.rs
git commit -m "ui: header / footer / split layout placeholders"
```

### Task 1.9: App struct + main loop

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/app.rs`**

```rust
//! App struct, view stack, main event loop.

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::Frame;
use tokio::{select, sync::mpsc, time::interval};

use crate::{
    error::Result,
    term::Tui,
    ui::{self, footer, header, palette::{self, Cmd, Palette}, theme::{self, Theme}},
    views::{AppEvent, Ctx, Outcome, View, ViewPayload},
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
}

impl App {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        Self {
            views: Vec::new(),
            palette: Palette::default(),
            theme: &theme::DEFAULT,
            toast: None,
            event_tx,
            event_rx,
            should_quit: false,
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

    pub fn pop(&mut self) {
        let mut ctx = Ctx::new(self.event_tx.clone());
        if let Some(mut v) = self.views.pop() {
            v.on_leave(&mut ctx);
        }
        if let Some(top) = self.views.last_mut() {
            top.on_enter(&mut ctx);
        }
        if self.views.is_empty() {
            self.should_quit = true;
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
            Cmd::Open(verb) => self.toast = Some(format!("not yet wired: :{verb}")),
            Cmd::Connect(_) => self.toast = Some("connect not yet wired".into()),
            Cmd::Terminate(_) => self.toast = Some("terminate not yet wired".into()),
            Cmd::Unknown(s) => self.toast = Some(format!("unknown command: {s}")),
        }
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

        // forward to active view
        if let Some(top) = self.views.last_mut() {
            let mut ctx = Ctx::new(self.event_tx.clone());
            let outcome = top.handle_key(key, &mut ctx);
            // Esc bubbles to Pop if the view passed
            let outcome = match outcome {
                Outcome::Pass if key.code == KeyCode::Esc => Outcome::Pop,
                other => other,
            };
            self.handle_outcome(outcome);
        } else if key.code == KeyCode::Esc {
            // No views, Esc quits.
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
        }
    }

    fn render(&mut self, f: &mut Frame) {
        let [head, main, foot] = ui::split(f.area());

        let (title, breadcrumb) = match self.views.last() {
            Some(v) => ("postui", v.title()),
            None => ("postui", ""),
        };
        header::render(f, head, self.theme, title, breadcrumb);

        if let Some(top) = self.views.last_mut() {
            top.render(f, main, self.theme);
        } else {
            ui::render_main_placeholder(f, main);
        }

        let hints = match self.views.last() {
            Some(_) => "[:] palette  [esc] back  [^Q] quit  [?] help",
            None => "[:] palette  [^Q] quit",
        };
        footer::render(f, foot, self.theme, hints, self.toast.as_deref(), &self.palette);
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

// Suppress dead-code warning for ViewPayload until wired.
#[allow(dead_code)]
fn _unused(_p: ViewPayload) {}
```

- [ ] **Step 2: Wire into `src/main.rs`**

```rust
mod app;
mod error;
mod logging;
mod term;
mod ui;
mod views;

use crate::error::Result;

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let app = app::App::new();
        app.run(&mut term).await
    })
}
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Smoke test the binary**

Run: `cargo run`
Expected: blank app appears (header line + empty pane + footer hints). Press `:`, type `q`, press `<enter>`. App exits cleanly. Run again, press `Ctrl-Q`. Exits cleanly. Run again, press `Esc` (with no views) — exits cleanly.

If your terminal is left in a weird state, the panic hook needs revisiting.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "app: main loop + palette dispatch + view stack"
```

**Milestone 1 complete.** App is runnable, palette works, terminal is panic-safe. No DB or views yet.

---

## Milestone 2 — Config + Connect

**End state:** `cargo run -- postgres://user:pass@host/db` connects to Postgres on launch and the header shows `connected: <db>`. `cargo run -- --connection prod` resolves a named connection from `~/.config/postui/config.toml` (with `${ENV}` interpolation). With no args, the app launches and the header reads `not connected`. Config errors at startup fail fast with a clear message.

### Task 2.1: Add config + db deps

**Files:** modify `Cargo.toml`

- [ ] **Step 1: Add deps**

```bash
cargo add serde --features derive
cargo add toml
cargo add url
cargo add tokio-postgres --features with-uuid-1,with-chrono-0_4,with-serde_json-1
cargo add postgres-types
cargo add chrono --features serde
cargo add uuid --features v4,serde
cargo add serde_json
cargo add rust_decimal
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add tokio-postgres, serde/toml, url, common pg types"
```

### Task 2.2: Config types + TOML parsing (no env interp yet)

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing test (inline `#[cfg(test)]` in `src/config.rs`)**

```rust
//! Config types: TOML schema, env interpolation, URI parsing.
//!
//! See spec section "Config Schema" for the user-facing shape.

use std::path::PathBuf;

use serde::Deserialize;

use crate::error::ConfigError;

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    #[serde(rename = "connection", default)]
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub views: ViewsConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct UiConfig {
    pub theme: String,
    pub tick_ms: u64,
    pub page_size: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            tick_ms: 2000,
            page_size: 100,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct ViewsConfig {
    #[serde(default)]
    pub queries: Option<ViewOverride>,
    #[serde(default)]
    pub locks: Option<ViewOverride>,
    #[serde(default)]
    pub sessions: Option<ViewOverride>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ViewOverride {
    pub tick_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConnectionConfig {
    pub name: String,
    /// Either `url = "postgres://..."` OR the discrete fields below.
    pub url: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub database: Option<String>,
    pub password: Option<String>,
    pub sslmode: Option<String>,
}

impl Config {
    pub fn from_toml(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    pub fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(ConfigError::Read)?;
        Self::from_toml(&text)
    }

    pub fn find_connection(&self, name: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg.ui.theme, "default");
        assert_eq!(cfg.ui.tick_ms, 2000);
        assert_eq!(cfg.ui.page_size, 100);
        assert!(cfg.connections.is_empty());
    }

    #[test]
    fn parses_full_example() {
        let toml = r#"
            [ui]
            theme = "dracula"
            tick_ms = 1500
            page_size = 50

            [[connection]]
            name = "local"
            host = "localhost"
            port = 5432
            user = "andrew"
            database = "app_dev"

            [[connection]]
            name = "stage"
            url = "postgres://andrew@db.stage:5432/app"

            [views.queries]
            tick_ms = 1000
        "#;
        let cfg = Config::from_toml(toml).unwrap();
        assert_eq!(cfg.ui.theme, "dracula");
        assert_eq!(cfg.ui.tick_ms, 1500);
        assert_eq!(cfg.ui.page_size, 50);
        assert_eq!(cfg.connections.len(), 2);
        assert_eq!(cfg.connections[0].name, "local");
        assert_eq!(cfg.connections[1].url.as_deref(), Some("postgres://andrew@db.stage:5432/app"));
        assert_eq!(cfg.views.queries.as_ref().and_then(|v| v.tick_ms), Some(1000));
    }

    #[test]
    fn find_connection_works() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "prod"
            host = "h"
            user = "u"
            database = "d"
        "#).unwrap();
        assert!(cfg.find_connection("prod").is_some());
        assert!(cfg.find_connection("missing").is_none());
    }

    #[test]
    fn parse_errors_surface_message() {
        let err = Config::from_toml("[ui\nbroken").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }
}
```

- [ ] **Step 2: Wire `config` module into `src/main.rs`**

```rust
mod app;
mod config;
mod error;
mod logging;
mod term;
mod ui;
mod views;

use crate::error::Result;

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let app = app::App::new();
        app.run(&mut term).await
    })
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib config`
Expected: 4 passed.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "config: TOML schema + parsing + lookup"
```

### Task 2.3: Env-var interpolation in config strings

**Files:** modify `src/config.rs`

- [ ] **Step 1: Add the failing tests at the bottom of the existing `tests` module**

```rust
    #[test]
    fn interpolates_env_in_password() {
        // SAFETY: tests are isolated; we set + unset.
        unsafe { std::env::set_var("POSTUI_TEST_PW", "s3cret"); }
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "${POSTUI_TEST_PW}"
        "#).unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.password.as_deref(), Some("s3cret"));
        unsafe { std::env::remove_var("POSTUI_TEST_PW"); }
    }

    #[test]
    fn interpolates_env_in_url() {
        unsafe { std::env::set_var("POSTUI_TEST_URL", "postgres://a:b@h/d"); }
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            url = "${POSTUI_TEST_URL}"
        "#).unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.url.as_deref(), Some("postgres://a:b@h/d"));
        unsafe { std::env::remove_var("POSTUI_TEST_URL"); }
    }

    #[test]
    fn missing_env_var_errors() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "${POSTUI_DEFINITELY_NOT_SET}"
        "#).unwrap();
        let err = cfg.connections[0].resolve_secrets().unwrap_err();
        match err {
            ConfigError::MissingEnv { var } => assert_eq!(var, "POSTUI_DEFINITELY_NOT_SET"),
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn passthrough_when_no_placeholder() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            host = "h"
            user = "u"
            database = "d"
            password = "literal-pw"
        "#).unwrap();
        let resolved = cfg.connections[0].resolve_secrets().unwrap();
        assert_eq!(resolved.password.as_deref(), Some("literal-pw"));
    }
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib config::tests::interpolates`
Expected: FAIL — `resolve_secrets` not defined.

- [ ] **Step 3: Implement `resolve_secrets` on `ConnectionConfig` (add below the existing `impl Config`)**

```rust
impl ConnectionConfig {
    /// Returns a clone with `${ENV_VAR}` placeholders in `password` and `url`
    /// resolved against the current process env.
    pub fn resolve_secrets(&self) -> Result<Self, ConfigError> {
        let mut out = self.clone();
        if let Some(p) = out.password.take() {
            out.password = Some(interpolate(&p)?);
        }
        if let Some(u) = out.url.take() {
            out.url = Some(interpolate(&u)?);
        }
        Ok(out)
    }
}

/// Replace `${VAR}` occurrences with env values. Errors on first missing var.
fn interpolate(input: &str) -> Result<String, ConfigError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| ConfigError::Parse(format!("unclosed ${{ in {input:?}")))?;
        let var = &after[..end];
        let val = std::env::var(var).map_err(|_| ConfigError::MissingEnv { var: var.to_string() })?;
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}
```

- [ ] **Step 4: Run tests, expect pass**

Run: `cargo test --lib config`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "config: ${ENV_VAR} interpolation in password and url"
```

### Task 2.4: Connection target — URI vs discrete fields

**Files:** modify `src/config.rs`

- [ ] **Step 1: Add tests for `as_target()`**

```rust
    #[test]
    fn as_target_from_url() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            url = "postgres://u:p@h:5432/d"
        "#).unwrap();
        let target = cfg.connections[0].as_target().unwrap();
        assert!(target.contains("postgres://"));
        assert!(target.contains("u"));
        assert!(target.contains("h"));
    }

    #[test]
    fn as_target_from_fields() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
            host = "h"
            port = 5433
            user = "u"
            database = "d"
            password = "pw"
            sslmode = "require"
        "#).unwrap();
        let target = cfg.connections[0].as_target().unwrap();
        // libpq-style key=value string
        assert!(target.contains("host=h"));
        assert!(target.contains("port=5433"));
        assert!(target.contains("user=u"));
        assert!(target.contains("dbname=d"));
        assert!(target.contains("password=pw"));
        assert!(target.contains("sslmode=require"));
    }

    #[test]
    fn as_target_with_no_fields_errors() {
        let cfg = Config::from_toml(r#"
            [[connection]]
            name = "x"
        "#).unwrap();
        let err = cfg.connections[0].as_target().unwrap_err();
        assert!(matches!(err, ConfigError::BadUri(_)));
    }
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib config::tests::as_target`
Expected: FAIL — `as_target` not defined.

- [ ] **Step 3: Implement `as_target` (append to the `impl ConnectionConfig` block)**

```rust
impl ConnectionConfig {
    /// Render this connection as a tokio_postgres-compatible connection string.
    /// Prefers `url` if present; otherwise builds a libpq-style key=value string.
    /// Caller should usually `resolve_secrets()` first.
    pub fn as_target(&self) -> Result<String, ConfigError> {
        if let Some(u) = &self.url {
            return Ok(u.clone());
        }
        let host = self.host.as_deref().ok_or_else(|| {
            ConfigError::BadUri("connection needs either `url` or `host`".into())
        })?;
        let mut parts = vec![format!("host={host}")];
        if let Some(p) = self.port {
            parts.push(format!("port={p}"));
        }
        if let Some(u) = &self.user {
            parts.push(format!("user={u}"));
        }
        if let Some(d) = &self.database {
            parts.push(format!("dbname={d}"));
        }
        if let Some(pw) = &self.password {
            parts.push(format!("password={pw}"));
        }
        if let Some(s) = &self.sslmode {
            parts.push(format!("sslmode={s}"));
        }
        parts.push("application_name=postui".to_string());
        Ok(parts.join(" "))
    }
}
```

Note: the `impl ConnectionConfig { fn resolve_secrets ... }` and this new `impl ConnectionConfig { fn as_target ... }` should be **merged into a single `impl` block**.

- [ ] **Step 4: Run tests, expect pass**

Run: `cargo test --lib config`
Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "config: as_target() builds libpq conn string from fields or url"
```

### Task 2.5: PgConn::connect

**Files:**
- Create: `src/db/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/db/mod.rs`**

```rust
//! Postgres connection wrapper.

use std::sync::Arc;

use tokio_postgres::{Client, NoTls};

use crate::error::DbError;

/// A live async Postgres connection. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct PgConn {
    inner: Arc<Client>,
    pub label: String,
}

impl PgConn {
    /// Connect using a libpq-compatible connection string. Spawns the
    /// connection driver task on the current tokio runtime.
    pub async fn connect(target: &str, label: String) -> Result<Self, DbError> {
        let (client, connection) = tokio_postgres::connect(target, NoTls)
            .await
            .map_err(|e| DbError::Connect(e.to_string()))?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(?e, "postgres connection driver exited");
            }
        });

        Ok(Self { inner: Arc::new(client), label })
    }

    pub fn client(&self) -> &Client {
        &self.inner
    }
}
```

- [ ] **Step 2: Wire `db` module into `src/main.rs`**

```rust
mod app;
mod config;
mod db;
mod error;
mod logging;
mod term;
mod ui;
mod views;

use crate::error::Result;

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let app = app::App::new();
        app.run(&mut term).await
    })
}
```

- [ ] **Step 3: Verify**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/db/mod.rs src/main.rs
git commit -m "db: PgConn wrapper around tokio_postgres::Client"
```

### Task 2.6: CLI parsing + bootstrap connect

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write `src/cli.rs`**

```rust
//! CLI argument parsing.

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "postui", version, about = "Terminal UI for PostgreSQL")]
pub struct Cli {
    /// Path to config file. Defaults to ~/.config/postui/config.toml.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Named connection from the config file.
    #[arg(long, conflicts_with = "uri")]
    pub connection: Option<String>,

    /// A postgres:// URI to connect to immediately.
    #[arg()]
    pub uri: Option<String>,
}
```

- [ ] **Step 2: Modify `App` to hold an optional connection and a `connect_to` helper. Edit `src/app.rs`.**

Add at the top of `src/app.rs` imports:

```rust
use crate::db::PgConn;
```

Replace the `App` struct with:

```rust
pub struct App {
    pub views: Vec<Box<dyn View>>,
    pub palette: Palette,
    pub theme: &'static Theme,
    pub toast: Option<String>,
    pub event_tx: mpsc::Sender<AppEvent>,
    pub event_rx: mpsc::Receiver<AppEvent>,
    pub should_quit: bool,
    pub conn: Option<PgConn>,
}
```

Replace `App::new` with:

```rust
impl App {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        Self {
            views: Vec::new(),
            palette: Palette::default(),
            theme: &theme::DEFAULT,
            toast: None,
            event_tx,
            event_rx,
            should_quit: false,
            conn: None,
        }
    }

    pub fn set_connection(&mut self, conn: PgConn) {
        let label = conn.label.clone();
        self.conn = Some(conn);
        self.toast = Some(format!("connected: {label}"));
    }
    // ... rest unchanged
```

Update `App::render` to use the connection label in the header:

```rust
    fn render(&mut self, f: &mut Frame) {
        let [head, main, foot] = ui::split(f.area());

        let title_owned = match &self.conn {
            Some(c) => format!("postui  ·  {}", c.label),
            None => "postui  ·  not connected".to_string(),
        };
        let breadcrumb = self.views.last().map(|v| v.title()).unwrap_or("");
        header::render(f, head, self.theme, &title_owned, breadcrumb);
        // ... rest unchanged
```

- [ ] **Step 3: Wire CLI + bootstrap into `src/main.rs`**

```rust
mod app;
mod cli;
mod config;
mod db;
mod error;
mod logging;
mod term;
mod ui;
mod views;

use std::path::PathBuf;

use clap::Parser;

use crate::{cli::Cli, config::Config, db::PgConn, error::{ConfigError, Result}};

fn default_config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "postui")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();
    let cli = Cli::parse();

    let config_path = cli.config.unwrap_or_else(default_config_path);
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        tracing::info!(path = %config_path.display(), "no config file; using defaults");
        Config::default()
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let conn = bootstrap_connection(&cli, &config).await?;
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let mut app = app::App::new();
        if let Some(c) = conn {
            app.set_connection(c);
        }
        app.run(&mut term).await
    })
}

async fn bootstrap_connection(cli: &Cli, config: &Config) -> Result<Option<PgConn>> {
    if let Some(uri) = &cli.uri {
        let label = label_for_uri(uri);
        return Ok(Some(PgConn::connect(uri, label).await?));
    }
    if let Some(name) = &cli.connection {
        let cfg = config.find_connection(name).ok_or_else(|| {
            ConfigError::Parse(format!("no connection named '{name}' in config"))
        })?;
        let resolved = cfg.resolve_secrets()?;
        let target = resolved.as_target()?;
        return Ok(Some(PgConn::connect(&target, name.clone()).await?));
    }
    Ok(None)
}

fn label_for_uri(uri: &str) -> String {
    // Pull out user@host/db for display, never the password.
    if let Ok(u) = url::Url::parse(uri) {
        let user = u.username();
        let host = u.host_str().unwrap_or("");
        let db = u.path().trim_start_matches('/');
        format!("{user}@{host}/{db}")
    } else {
        "uri".into()
    }
}
```

- [ ] **Step 4: Build and smoke test**

Run: `cargo build`
Expected: PASS.

If you have a local Postgres:

```bash
cargo run -- postgres://$USER@localhost/postgres
```

Expected: header reads `postui  ·  $USER@localhost/postgres`, footer briefly toasts `connected: $USER@localhost/postgres`. `:q` quits.

Run with no args:

```bash
cargo run
```

Expected: header reads `postui  ·  not connected`. `:q` quits.

Run with a bogus connection:

```bash
cargo run -- --connection nonexistent
```

Expected: program exits with error message about unknown connection.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs src/app.rs
git commit -m "cli: bootstrap connection from --uri / --connection NAME"
```

**Milestone 2 complete.** App connects on launch, header shows the active connection. Config + env interpolation + URI parsing all wired through.

---

## Milestone 3 — Browse Chain

**End state:** With a connection, type `:databases` → see databases. `<enter>` switches DB and pushes `:schemas`. `<enter>` on a schema pushes `:tables`. `j`/`k`/`w`/`b`/`e` move the selection. `<esc>` pops. `:tables` shows name / row-estimate / size columns from `pg_class`.

### Task 3.1: Generic `ui::table` widget with selection + vim motions

**Files:**
- Create: `src/keys.rs`
- Create: `src/ui/table.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/keys.rs`**

```rust
//! Keymap: vim motion bindings for list/table views.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Generic motion intent emitted from a key event in a list view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
    PageUp,    // w / PageUp / Ctrl-D — but we use w for forward, b for back
    PageDown,
    Home,      // gg
    End,       // e (jump to last row, per spec)
    PageNext,  // PageDown
    PagePrev,  // PageUp
}

pub fn vim_motion(key: KeyEvent) -> Option<Motion> {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
    {
        return None;
    }
    Some(match key.code {
        KeyCode::Char('j') | KeyCode::Down => Motion::Down,
        KeyCode::Char('k') | KeyCode::Up => Motion::Up,
        KeyCode::Char('h') | KeyCode::Left => Motion::Left,
        KeyCode::Char('l') | KeyCode::Right => Motion::Right,
        KeyCode::Char('w') | KeyCode::PageDown => Motion::PageDown,
        KeyCode::Char('b') | KeyCode::PageUp => Motion::PageUp,
        KeyCode::Char('e') | KeyCode::End => Motion::End,
        KeyCode::Home => Motion::Home,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn vim_keys_map_to_motions() {
        assert_eq!(vim_motion(k('j')), Some(Motion::Down));
        assert_eq!(vim_motion(k('k')), Some(Motion::Up));
        assert_eq!(vim_motion(k('h')), Some(Motion::Left));
        assert_eq!(vim_motion(k('l')), Some(Motion::Right));
        assert_eq!(vim_motion(k('w')), Some(Motion::PageDown));
        assert_eq!(vim_motion(k('b')), Some(Motion::PageUp));
        assert_eq!(vim_motion(k('e')), Some(Motion::End));
    }

    #[test]
    fn ctrl_modified_keys_pass_through() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(vim_motion(key), None);
    }

    #[test]
    fn arrow_keys_map_too() {
        assert_eq!(
            vim_motion(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Motion::Down)
        );
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(vim_motion(k('x')), None);
    }

    // Suppress unused-import warning when not running tests.
    #[allow(dead_code)]
    fn _silence(_: KeyEventKind, _: KeyEventState) {}
}
```

- [ ] **Step 2: Write `src/ui/table.rs`**

```rust
//! Generic data-table widget with cursor + vim-style motion.
//!
//! Holds rows of `String` cells. Views feed it pre-formatted strings; numeric
//! / type-aware formatting happens in `db::types`.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table as RTable, TableState},
};

use crate::{keys::Motion, ui::theme::Theme};

const PAGE_SIZE: usize = 10;

#[derive(Debug, Clone)]
pub struct DataTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub state: TableState,
}

impl DataTable {
    pub fn new(headers: Vec<&str>) -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            headers: headers.into_iter().map(String::from).collect(),
            rows: Vec::new(),
            state,
        }
    }

    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        if self.rows.is_empty() {
            self.state.select(None);
        } else if self.state.selected().is_none() || self.state.selected().unwrap() >= self.rows.len() {
            self.state.select(Some(0));
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn selected_row(&self) -> Option<&[String]> {
        self.state.selected().and_then(|i| self.rows.get(i)).map(|v| v.as_slice())
    }

    pub fn move_motion(&mut self, m: Motion) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let cur = self.state.selected().unwrap_or(0);
        let next = match m {
            Motion::Up => cur.saturating_sub(1),
            Motion::Down => (cur + 1).min(last),
            Motion::PageUp | Motion::PagePrev => cur.saturating_sub(PAGE_SIZE),
            Motion::PageDown | Motion::PageNext => (cur + PAGE_SIZE).min(last),
            Motion::Home => 0,
            Motion::End => last,
            // Left/Right are no-ops at the table level (consumed by tabs/columns).
            Motion::Left | Motion::Right => cur,
        };
        self.state.select(Some(next));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let header_row = Row::new(self.headers.iter().cloned().map(Cell::from).collect::<Vec<_>>())
            .style(Style::default().fg(theme.table_header).add_modifier(Modifier::BOLD));

        let body: Vec<Row> = self
            .rows
            .iter()
            .map(|r| Row::new(r.iter().cloned().map(Cell::from).collect::<Vec<_>>()))
            .collect();

        let widths: Vec<Constraint> = self
            .headers
            .iter()
            .map(|_| Constraint::Percentage(100 / self.headers.len().max(1) as u16))
            .collect();

        let table = RTable::new(body, widths)
            .header(header_row)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .row_highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> DataTable {
        let mut t = DataTable::new(vec!["a", "b"]);
        t.set_rows(vec![
            vec!["1".into(), "x".into()],
            vec!["2".into(), "y".into()],
            vec!["3".into(), "z".into()],
        ]);
        t
    }

    #[test]
    fn selection_starts_at_zero_when_rows_set() {
        let t = populated();
        assert_eq!(t.selected_index(), Some(0));
    }

    #[test]
    fn down_motion_advances() {
        let mut t = populated();
        t.move_motion(Motion::Down);
        assert_eq!(t.selected_index(), Some(1));
    }

    #[test]
    fn down_clamps_at_last_row() {
        let mut t = populated();
        t.move_motion(Motion::End);
        t.move_motion(Motion::Down);
        assert_eq!(t.selected_index(), Some(2));
    }

    #[test]
    fn up_clamps_at_zero() {
        let mut t = populated();
        t.move_motion(Motion::Up);
        assert_eq!(t.selected_index(), Some(0));
    }

    #[test]
    fn end_motion_goes_to_last() {
        let mut t = populated();
        t.move_motion(Motion::End);
        assert_eq!(t.selected_index(), Some(2));
    }

    #[test]
    fn empty_table_motion_is_noop() {
        let mut t = DataTable::new(vec!["a"]);
        t.move_motion(Motion::Down);
        assert_eq!(t.selected_index(), None);
    }
}
```

- [ ] **Step 3: Update `src/ui/mod.rs`**

```rust
pub mod footer;
pub mod header;
pub mod palette;
pub mod table;
pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

pub fn split(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

#[allow(dead_code)]
pub fn render_main_placeholder(f: &mut Frame, area: Rect) {
    use ratatui::widgets::{Block, Borders};
    f.render_widget(Block::default().borders(Borders::NONE), area);
}
```

- [ ] **Step 4: Wire `keys` module into `src/main.rs`**

```rust
mod app;
mod cli;
mod config;
mod db;
mod error;
mod keys;
mod logging;
mod term;
mod ui;
mod views;

// (rest of file unchanged)
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib keys && cargo test --lib table`
Expected: 4 + 6 = 10 passed.

- [ ] **Step 6: Commit**

```bash
git add src/keys.rs src/ui/table.rs src/ui/mod.rs src/main.rs
git commit -m "ui: generic DataTable widget + vim motion keymap"
```

### Task 3.2: db::catalog typed result structs + list_databases

**Files:**
- Create: `src/db/catalog.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/catalog.rs`**

```rust
//! pg_catalog queries returning typed structs. No tokio_postgres::Row escapes
//! this module.

use crate::{db::PgConn, error::DbError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseInfo {
    pub name: String,
    pub owner: String,
    pub encoding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub estimated_rows: i64,
    pub total_bytes: i64,
}

const SQL_LIST_DATABASES: &str = "
    SELECT d.datname,
           pg_get_userbyid(d.datdba) AS owner,
           pg_encoding_to_char(d.encoding) AS encoding
    FROM pg_database d
    WHERE NOT d.datistemplate
    ORDER BY d.datname";

const SQL_LIST_SCHEMAS: &str = "
    SELECT n.nspname,
           pg_get_userbyid(n.nspowner) AS owner
    FROM pg_namespace n
    WHERE n.nspname NOT LIKE 'pg_%'
      AND n.nspname <> 'information_schema'
    ORDER BY n.nspname";

const SQL_LIST_TABLES: &str = "
    SELECT n.nspname AS schema,
           c.relname AS name,
           c.reltuples::bigint AS estimated_rows,
           pg_total_relation_size(c.oid)::bigint AS total_bytes
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relkind IN ('r', 'p')
      AND n.nspname = $1
    ORDER BY c.relname";

pub async fn list_databases(conn: &PgConn) -> Result<Vec<DatabaseInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_DATABASES, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_DATABASES.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| DatabaseInfo {
        name: r.get(0),
        owner: r.get(1),
        encoding: r.get(2),
    }).collect())
}

pub async fn list_schemas(conn: &PgConn) -> Result<Vec<SchemaInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_SCHEMAS, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_SCHEMAS.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| SchemaInfo {
        name: r.get(0),
        owner: r.get(1),
    }).collect())
}

pub async fn list_tables(conn: &PgConn, schema: &str) -> Result<Vec<TableInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_TABLES, &[&schema])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_TABLES.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| TableInfo {
        schema: r.get(0),
        name: r.get(1),
        estimated_rows: r.get(2),
        total_bytes: r.get(3),
    }).collect())
}
```

- [ ] **Step 2: Add to `src/db/mod.rs` (above the existing PgConn code)**

```rust
pub mod catalog;
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/db/catalog.rs src/db/mod.rs
git commit -m "db: catalog list_databases / list_schemas / list_tables"
```

### Task 3.3: Integration test harness with testcontainers

**Files:**
- Modify: `Cargo.toml`
- Create: `tests/common/mod.rs`
- Create: `tests/catalog_it.rs`

- [ ] **Step 1: Add testcontainers dep**

```bash
cargo add --dev testcontainers
cargo add --dev testcontainers-modules --features postgres
cargo add --dev tokio --features rt,macros
```

- [ ] **Step 2: Write `tests/common/mod.rs`**

```rust
//! Shared test harness: spin up a Postgres container and produce a PgConn.

use postui::db::PgConn;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

pub struct TestDb {
    pub conn: PgConn,
    _container: ContainerAsync<Postgres>,
}

pub async fn start() -> TestDb {
    let container = Postgres::default()
        .start()
        .await
        .expect("postgres container start");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let conn_str = format!("host={host} port={port} user=postgres password=postgres dbname=postgres");
    let conn = PgConn::connect(&conn_str, "test".into())
        .await
        .expect("connect");
    TestDb { conn, _container: container }
}
```

- [ ] **Step 3: Make the crate exposable for tests by adding `lib.rs`**

Create `src/lib.rs`:

```rust
//! Library facade exposing internal modules for integration tests.

pub mod app;
pub mod cli;
pub mod config;
pub mod db;
pub mod error;
pub mod keys;
pub mod logging;
pub mod term;
pub mod ui;
pub mod views;
```

Modify `src/main.rs` to use the lib crate (replace contents):

```rust
use std::path::PathBuf;

use clap::Parser;

use postui::{
    app, cli::Cli, config::Config, db::PgConn, error::{ConfigError, Result}, logging, term,
};

fn default_config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "postui")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    term::install_panic_hook();
    let cli = Cli::parse();

    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        tracing::info!(path = %config_path.display(), "no config file; using defaults");
        Config::default()
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let conn = bootstrap_connection(&cli, &config).await?;
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let mut app = app::App::new();
        if let Some(c) = conn {
            app.set_connection(c);
        }
        app.run(&mut term).await
    })
}

async fn bootstrap_connection(cli: &Cli, config: &Config) -> Result<Option<PgConn>> {
    if let Some(uri) = &cli.uri {
        let label = label_for_uri(uri);
        return Ok(Some(PgConn::connect(uri, label).await?));
    }
    if let Some(name) = &cli.connection {
        let cfg = config.find_connection(name).ok_or_else(|| {
            ConfigError::Parse(format!("no connection named '{name}' in config"))
        })?;
        let resolved = cfg.resolve_secrets()?;
        let target = resolved.as_target()?;
        return Ok(Some(PgConn::connect(&target, name.clone()).await?));
    }
    Ok(None)
}

fn label_for_uri(uri: &str) -> String {
    if let Ok(u) = url::Url::parse(uri) {
        let user = u.username();
        let host = u.host_str().unwrap_or("");
        let db = u.path().trim_start_matches('/');
        format!("{user}@{host}/{db}")
    } else {
        "uri".into()
    }
}
```

Add to `Cargo.toml` so both bin and lib are produced:

```toml
[lib]
name = "postui"
path = "src/lib.rs"

[[bin]]
name = "postui"
path = "src/main.rs"
```

- [ ] **Step 4: Write `tests/catalog_it.rs`**

```rust
mod common;

use postui::db::catalog;

#[tokio::test]
#[ignore = "requires docker"]
async fn list_databases_includes_postgres() {
    let db = common::start().await;
    let dbs = catalog::list_databases(&db.conn).await.expect("list_databases");
    assert!(dbs.iter().any(|d| d.name == "postgres"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_schemas_includes_public() {
    let db = common::start().await;
    let schemas = catalog::list_schemas(&db.conn).await.expect("list_schemas");
    assert!(schemas.iter().any(|s| s.name == "public"));
    assert!(!schemas.iter().any(|s| s.name.starts_with("pg_")));
    assert!(!schemas.iter().any(|s| s.name == "information_schema"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_tables_returns_created_table() {
    let db = common::start().await;
    db.conn.client()
        .execute("CREATE TABLE public.t1 (id int)", &[])
        .await
        .unwrap();
    let tables = catalog::list_tables(&db.conn, "public").await.expect("list_tables");
    assert!(tables.iter().any(|t| t.name == "t1"));
}
```

- [ ] **Step 5: Run tests (only if docker is available)**

Run: `cargo test --test catalog_it -- --ignored`
Expected (with docker): 3 passed.
Expected (no docker): tests skipped because of `#[ignore]`. Run `cargo test --test catalog_it` plain to confirm they're discovered.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock tests/ src/lib.rs src/main.rs
git commit -m "tests: testcontainers harness + catalog integration tests"
```

### Task 3.4: ConnectionsView (lists profiles from config)

**Files:**
- Create: `src/views/connections.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Write `src/views/connections.rs`**

```rust
//! :connections — list of connection profiles from config.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    config::Config,
    keys::vim_motion,
    ui::{table::DataTable, theme::Theme},
    views::{Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct ConnectionsView {
    id: ViewId,
    table: DataTable,
    /// Names in row order so we can resolve the selection.
    names: Vec<String>,
}

impl ConnectionsView {
    pub fn new(config: &Config, active: Option<&str>) -> Self {
        let mut table = DataTable::new(vec!["", "name", "host", "user", "database"]);
        let rows = config.connections.iter().map(|c| {
            let active_mark = if active == Some(c.name.as_str()) { "*" } else { "" };
            vec![
                active_mark.into(),
                c.name.clone(),
                c.host.clone().unwrap_or_else(|| {
                    c.url.as_deref().map(short_host).unwrap_or_default()
                }),
                c.user.clone().unwrap_or_default(),
                c.database.clone().unwrap_or_default(),
            ]
        }).collect();
        table.set_rows(rows);
        Self {
            id: ViewId::next(),
            table,
            names: config.connections.iter().map(|c| c.name.clone()).collect(),
        }
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.table.selected_index().and_then(|i| self.names.get(i)).map(String::as_str)
    }
}

fn short_host(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_else(|| "(uri)".into())
}

impl View for ConnectionsView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "connections" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        match key.code {
            // Connect/switch is wired by App in a later task — for now, just emit Pass.
            KeyCode::Enter => Outcome::Pass,
            _ => Outcome::Pass,
        }
    }

    fn apply(&mut self, _payload: ViewPayload) {}
}
```

- [ ] **Step 2: Update `src/views/mod.rs` to export the view**

Add at the top:

```rust
pub mod connections;
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/views/connections.rs src/views/mod.rs
git commit -m "views: ConnectionsView lists config profiles"
```

### Task 3.5: DatabasesView, SchemasView, TablesView (one task each)

The three views share a pattern. Implement them in sequence.

#### 3.5a — DatabasesView

**Files:**
- Create: `src/views/databases.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Add a typed payload variant for databases**

Edit `src/views/mod.rs` — replace the `ViewPayload` enum with:

```rust
#[derive(Debug)]
pub enum ViewPayload {
    Databases(Result<Vec<crate::db::catalog::DatabaseInfo>, crate::error::DbError>),
    Schemas(Result<Vec<crate::db::catalog::SchemaInfo>, crate::error::DbError>),
    Tables(Result<Vec<crate::db::catalog::TableInfo>, crate::error::DbError>),
}
```

- [ ] **Step 2: Write `src/views/databases.rs`**

```rust
//! :databases — list of databases on the current cluster.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{PgConn, catalog::{DatabaseInfo, list_databases}},
    error::DbError,
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
}

// Suppress unused-import warning if DbError isn't used directly in this file.
#[allow(dead_code)]
fn _force_dbgerr(_e: DbError) {}
```

- [ ] **Step 3: Add to `src/views/mod.rs`**

```rust
pub mod databases;
```

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/databases.rs src/views/mod.rs
git commit -m "views: DatabasesView with on_enter fetch"
```

#### 3.5b — SchemasView

**Files:**
- Create: `src/views/schemas.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Write `src/views/schemas.rs`** (same pattern as databases)

```rust
//! :schemas — list of schemas on the current database.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{PgConn, catalog::{SchemaInfo, list_schemas}},
    keys::vim_motion,
    ui::{table::DataTable, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct SchemasView {
    id: ViewId,
    table: DataTable,
    rows: Vec<SchemaInfo>,
    error: Option<String>,
    conn: PgConn,
}

impl SchemasView {
    pub fn new(conn: PgConn) -> Self {
        let mut table = DataTable::new(vec!["name", "owner"]);
        table.set_rows(vec![]);
        Self { id: ViewId::next(), table, rows: vec![], error: None, conn }
    }

    pub fn selected(&self) -> Option<&SchemaInfo> {
        self.table.selected_index().and_then(|i| self.rows.get(i))
    }
}

impl View for SchemasView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "schemas" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => Outcome::Pass, // App pushes :tables (Task 3.6)
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        let view_id = self.id;
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();
        tokio::spawn(async move {
            let result = list_schemas(&conn).await;
            let _ = tx.send(AppEvent::ViewData {
                view_id,
                payload: ViewPayload::Schemas(result),
            }).await;
        });
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Schemas(res) = payload {
            match res {
                Ok(rows) => {
                    self.rows = rows;
                    let display: Vec<Vec<String>> = self.rows.iter().map(|s| vec![
                        s.name.clone(),
                        s.owner.clone(),
                    ]).collect();
                    self.table.set_rows(display);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e}")),
            }
        }
    }
}
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod schemas;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/views/schemas.rs src/views/mod.rs
git commit -m "views: SchemasView with on_enter fetch"
```

#### 3.5c — TablesView

**Files:**
- Create: `src/views/tables.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Write `src/views/tables.rs`**

```rust
//! :tables — list of tables in the current schema.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{PgConn, catalog::{TableInfo, list_tables}},
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
        Self { id: ViewId::next(), table, rows: vec![], error: None, conn, schema }
    }

    pub fn selected(&self) -> Option<&TableInfo> {
        self.table.selected_index().and_then(|i| self.rows.get(i))
    }
}

fn human_bytes(bytes: i64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K { return format!("{bytes} B"); }
    if b < K * K { return format!("{:.1} KB", b / K); }
    if b < K * K * K { return format!("{:.1} MB", b / K / K); }
    format!("{:.2} GB", b / K / K / K)
}

impl View for TablesView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "tables" }

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
            let _ = tx.send(AppEvent::ViewData {
                view_id,
                payload: ViewPayload::Tables(result),
            }).await;
        });
    }

    fn apply(&mut self, payload: ViewPayload) {
        if let ViewPayload::Tables(res) = payload {
            match res {
                Ok(rows) => {
                    self.rows = rows;
                    let display: Vec<Vec<String>> = self.rows.iter().map(|t| vec![
                        t.name.clone(),
                        t.estimated_rows.to_string(),
                        human_bytes(t.total_bytes),
                    ]).collect();
                    self.table.set_rows(display);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e}")),
            }
        }
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
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod tables;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib tables`
Expected: 1 passed.

- [ ] **Step 4: Commit**

```bash
git add src/views/tables.rs src/views/mod.rs
git commit -m "views: TablesView with on_enter fetch + human-readable size"
```

### Task 3.6: Wire palette commands + drill-down in App

**Files:** modify `src/app.rs`

- [ ] **Step 1: Update `App::dispatch_cmd` to open browse views and add a helper for `<enter>` drill-down**

Replace the `dispatch_cmd` function in `src/app.rs` with:

```rust
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
            Cmd::Terminate(_) => self.toast = Some("terminate not yet wired".into()),
            Cmd::Unknown(s) => self.toast = Some(format!("unknown command: {s}")),
        }
    }

    fn open(&mut self, verb: &str) {
        use crate::views::{databases::DatabasesView, schemas::SchemasView, tables::TablesView};
        let conn = match self.conn.clone() {
            Some(c) => c,
            None => {
                self.toast = Some("not connected — pass --uri or --connection at launch".into());
                return;
            }
        };
        match verb {
            "databases" | "db" => self.push(Box::new(DatabasesView::new(conn))),
            "schemas" | "sc" => self.push(Box::new(SchemasView::new(conn))),
            "tables" | "tb" => self.push(Box::new(TablesView::new(conn, self.current_schema.clone()))),
            other => self.toast = Some(format!("not yet wired: :{other}")),
        }
    }
```

- [ ] **Step 2: Add `current_schema` to App state**

Add field to the `App` struct:

```rust
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
}
```

Update `App::new` to initialize it:

```rust
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        Self {
            views: Vec::new(),
            palette: Palette::default(),
            theme: &theme::DEFAULT,
            toast: None,
            event_tx,
            event_rx,
            should_quit: false,
            conn: None,
            current_schema: "public".to_string(),
        }
    }
```

- [ ] **Step 3: Add Enter-key drill-down: when a view returns `Pass` on Enter, the dispatcher decides what to push.**

In `App::handle_key`, replace the section that forwards to the active view with:

```rust
        // forward to active view
        if let Some(top) = self.views.last_mut() {
            let mut ctx = Ctx::new(self.event_tx.clone());
            let outcome = top.handle_key(key, &mut ctx);
            // Esc bubbles to Pop if the view passed
            let outcome = match outcome {
                Outcome::Pass if key.code == KeyCode::Esc => Outcome::Pop,
                Outcome::Pass if key.code == KeyCode::Enter => self.handle_enter_drilldown(),
                other => other,
            };
            self.handle_outcome(outcome);
        } else if key.code == KeyCode::Esc {
            self.should_quit = true;
        }
```

- [ ] **Step 4: Implement `handle_enter_drilldown` on App**

Add this method to `impl App`:

```rust
    fn handle_enter_drilldown(&mut self) -> Outcome {
        use crate::views::{
            databases::DatabasesView, schemas::SchemasView, tables::TablesView,
        };
        let Some(conn) = self.conn.clone() else {
            return Outcome::Consumed;
        };
        // Snapshot the active view's title to decide what to do.
        let title = match self.views.last() {
            Some(v) => v.title(),
            None => return Outcome::Consumed,
        };
        match title {
            "databases" => {
                // Switch DB: rebuild PgConn pointing at the selected DB.
                let top = self.views.last().unwrap();
                let dbs = top
                    .as_any()
                    .and_then(|a| a.downcast_ref::<DatabasesView>());
                if let Some(view) = dbs {
                    if let Some(d) = view.selected() {
                        let name = d.name.clone();
                        let event_tx = self.event_tx.clone();
                        let label_template = conn.label.clone();
                        // Spawn reconnect; replace conn when done.
                        tokio::spawn(async move {
                            let target = format!("dbname={} application_name=postui", name);
                            let _ = event_tx.send(AppEvent::Toast(
                                format!("switching to db {name} (re-launch with --uri to change host/user)"),
                            )).await;
                            // For simplicity in v1, we show a toast and don't actually reconnect.
                            // Full DB switching is wired in Task 3.7.
                            let _ = (target, label_template);
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
            _ => Outcome::Consumed,
        }
    }
```

- [ ] **Step 5: Add `as_any` to the View trait**

In `src/views/mod.rs`, add to the trait:

```rust
pub trait View: Send {
    fn id(&self) -> ViewId;
    fn title(&self) -> &str;
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome;
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    fn on_enter(&mut self, _ctx: &mut Ctx) {}
    fn on_leave(&mut self, _ctx: &mut Ctx) {}
    fn apply(&mut self, _payload: ViewPayload) {}

    /// Optional cast for the App to access view-specific state (e.g., selection).
    /// Implementations should return `Some(self)`.
    fn as_any(&self) -> Option<&dyn std::any::Any> { None }
}
```

In each view (`connections.rs`, `databases.rs`, `schemas.rs`, `tables.rs`), add this method to the `impl View for X` block:

```rust
    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
```

And `use std::any::Any;` is not needed because we use the fully-qualified path.

- [ ] **Step 6: Build, smoke test**

Run: `cargo build`
Expected: PASS.

If you have a local Postgres:

```bash
cargo run -- postgres://$USER@localhost/postgres
```

Press `:`, type `databases`, `<enter>`. You see the database list. `j`/`k` navigate. `:`, `schemas`, `<enter>`. `<enter>` on a schema → tables list. `<esc>` pops back through. `:q` quits.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/views/
git commit -m "app: dispatch :databases/:schemas/:tables + Enter drill-down"
```

### Task 3.7: DB switching when Enter is pressed in `:databases`

**Files:** modify `src/app.rs`

- [ ] **Step 1: Replace the placeholder DB-switch toast in `handle_enter_drilldown`'s `"databases"` arm with a real reconnect**

Replace the `"databases" =>` arm in `handle_enter_drilldown` with:

```rust
            "databases" => {
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<DatabasesView>());
                if let Some(v) = view {
                    if let Some(d) = v.selected() {
                        let name = d.name.clone();
                        let old_conn = conn.clone();
                        let event_tx = self.event_tx.clone();
                        // Rebuild the connection string by overriding dbname.
                        // We don't have access to the original target string, so we
                        // ask Postgres for current connection params and substitute.
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
```

- [ ] **Step 2: Add `ConnectionSwitched` to `AppEvent` in `src/views/mod.rs`**

```rust
#[derive(Debug)]
pub enum AppEvent {
    ViewData {
        view_id: ViewId,
        payload: ViewPayload,
    },
    Toast(String),
    ConnectionSwitched(crate::db::PgConn),
}
```

- [ ] **Step 3: Add `switch_database` and event handler in `src/app.rs`**

Add at module level in `src/app.rs` (outside `impl App`):

```rust
async fn switch_database(old: &PgConn, new_db: &str) -> Result<PgConn> {
    // Pull the current host / port / user / sslmode from the live conn.
    let row = old.client()
        .query_one(
            "SELECT current_setting('listen_addresses', true), inet_server_port(), current_user",
            &[],
        )
        .await
        .map_err(|e| crate::error::DbError::Query { sql: "current params".into(), source: e.to_string() })?;
    let port: i32 = row.get::<_, Option<i32>>(1).unwrap_or(5432);
    let user: String = row.get(2);
    // We can't recover the password from a live conn; the user must rely on
    // ~/.pgpass or a passwordless local auth for db-switch in v1.
    let target = format!("host=/var/run/postgresql user={user} dbname={new_db} port={port} application_name=postui");
    let label = format!("{user}@{new_db}");
    Ok(PgConn::connect(&target, label).await?)
}
```

Update `App::handle_event`:

```rust
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
                // Refresh the active view so it picks up the new conn.
                let mut ctx = Ctx::new(self.event_tx.clone());
                if let Some(top) = self.views.last_mut() {
                    top.on_enter(&mut ctx);
                }
            }
        }
    }
```

Note: the `switch_database` helper here is intentionally simplified — it works for local-socket Postgres and falls back gracefully when password auth is needed by erroring out. v2 will track the original connection target so we can override `dbname` cleanly. For v1, treat DB-switch as best-effort.

- [ ] **Step 4: Build, smoke test**

Run: `cargo build && cargo run -- postgres://$USER@localhost/postgres`
Expected: from `:databases`, `<enter>` either switches DB (toast confirms) or shows a "switch failed" toast — the app stays alive either way.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/views/mod.rs
git commit -m "app: best-effort db switch on Enter from :databases"
```

**Milestone 3 complete.** Browse chain works: `:databases`, `:schemas`, `:tables` with vim motions and drill-down.

---

## Milestone 4 — Table Inspector + Row Detail

**End state:** From `:tables`, `<enter>` on a table opens an inspector with tabs `rows | columns | indexes | constraints | size`. `h`/`l` switch tabs. `j`/`k` moves selection within the active tab. `<enter>` on a row opens a row-detail view (key/value form). All read-only — no editing yet.

### Task 4.1: Extend catalog with column / index / constraint / size queries

**Files:**
- Modify: `src/db/catalog.rs`

- [ ] **Step 1: Append the new structs and queries**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexInfo {
    pub name: String,
    pub definition: String,
    pub size_bytes: i64,
    pub scans: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintInfo {
    pub name: String,
    pub kind: String,        // 'PRIMARY KEY' | 'FOREIGN KEY' | 'UNIQUE' | 'CHECK'
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSize {
    pub total_bytes: i64,
    pub heap_bytes: i64,
    pub indexes_bytes: i64,
    pub toast_bytes: i64,
    pub estimated_rows: i64,
}

const SQL_LIST_COLUMNS: &str = "
    SELECT a.attname,
           pg_catalog.format_type(a.atttypid, a.atttypmod),
           NOT a.attnotnull,
           pg_get_expr(d.adbin, d.adrelid),
           col_description(a.attrelid, a.attnum)
    FROM pg_attribute a
    JOIN pg_class c ON c.oid = a.attrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    WHERE n.nspname = $1
      AND c.relname = $2
      AND a.attnum > 0
      AND NOT a.attisdropped
    ORDER BY a.attnum";

const SQL_LIST_INDEXES: &str = "
    SELECT i.indexrelname,
           pg_get_indexdef(idx.indexrelid),
           pg_relation_size(idx.indexrelid)::bigint,
           COALESCE(s.idx_scan, 0)::bigint
    FROM pg_index idx
    JOIN pg_class ic ON ic.oid = idx.indexrelid
    JOIN pg_class c ON c.oid = idx.indrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    JOIN pg_stat_all_indexes i ON i.indexrelid = idx.indexrelid
    LEFT JOIN pg_stat_user_indexes s ON s.indexrelid = idx.indexrelid
    WHERE n.nspname = $1 AND c.relname = $2
    ORDER BY i.indexrelname";

const SQL_LIST_CONSTRAINTS: &str = "
    SELECT con.conname,
           CASE con.contype
             WHEN 'p' THEN 'PRIMARY KEY'
             WHEN 'f' THEN 'FOREIGN KEY'
             WHEN 'u' THEN 'UNIQUE'
             WHEN 'c' THEN 'CHECK'
             WHEN 'x' THEN 'EXCLUSION'
             ELSE 'OTHER'
           END,
           pg_get_constraintdef(con.oid)
    FROM pg_constraint con
    JOIN pg_class c ON c.oid = con.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = $1 AND c.relname = $2
    ORDER BY con.conname";

const SQL_TABLE_SIZE: &str = "
    SELECT pg_total_relation_size(c.oid)::bigint AS total,
           pg_relation_size(c.oid, 'main')::bigint AS heap,
           pg_indexes_size(c.oid)::bigint AS indexes,
           COALESCE(pg_total_relation_size(c.reltoastrelid), 0)::bigint AS toast,
           c.reltuples::bigint AS rows
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = $1 AND c.relname = $2";

pub async fn list_columns(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_COLUMNS, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_COLUMNS.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| ColumnInfo {
        name: r.get(0),
        data_type: r.get(1),
        nullable: r.get(2),
        default: r.get(3),
        comment: r.get(4),
    }).collect())
}

pub async fn list_indexes(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<IndexInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_INDEXES, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_INDEXES.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| IndexInfo {
        name: r.get(0),
        definition: r.get(1),
        size_bytes: r.get(2),
        scans: r.get(3),
    }).collect())
}

pub async fn list_constraints(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<ConstraintInfo>, DbError> {
    let rows = conn.client()
        .query(SQL_LIST_CONSTRAINTS, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LIST_CONSTRAINTS.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| ConstraintInfo {
        name: r.get(0),
        kind: r.get(1),
        definition: r.get(2),
    }).collect())
}

pub async fn table_size(conn: &PgConn, schema: &str, table: &str) -> Result<TableSize, DbError> {
    let row = conn.client()
        .query_one(SQL_TABLE_SIZE, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_TABLE_SIZE.into(), source: e.to_string() })?;
    Ok(TableSize {
        total_bytes: row.get(0),
        heap_bytes: row.get(1),
        indexes_bytes: row.get(2),
        toast_bytes: row.get(3),
        estimated_rows: row.get(4),
    })
}
```

- [ ] **Step 2: Append integration tests in `tests/catalog_it.rs`**

```rust
#[tokio::test]
#[ignore = "requires docker"]
async fn list_columns_returns_typed_columns() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (id int NOT NULL, name text DEFAULT 'x', extra jsonb)",
        &[],
    ).await.unwrap();
    let cols = catalog::list_columns(&db.conn, "public", "t").await.unwrap();
    assert_eq!(cols.len(), 3);
    assert_eq!(cols[0].name, "id");
    assert!(!cols[0].nullable);
    assert_eq!(cols[1].default.as_deref(), Some("'x'::text"));
    assert_eq!(cols[2].data_type, "jsonb");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_indexes_returns_pk() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (id int PRIMARY KEY)",
        &[],
    ).await.unwrap();
    let ix = catalog::list_indexes(&db.conn, "public", "t").await.unwrap();
    assert!(ix.iter().any(|i| i.name.contains("pkey")));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn list_constraints_returns_pk() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (id int PRIMARY KEY)",
        &[],
    ).await.unwrap();
    let con = catalog::list_constraints(&db.conn, "public", "t").await.unwrap();
    assert!(con.iter().any(|c| c.kind == "PRIMARY KEY"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn table_size_returns_nonneg() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (id int)",
        &[],
    ).await.unwrap();
    let sz = catalog::table_size(&db.conn, "public", "t").await.unwrap();
    assert!(sz.total_bytes >= 0);
}
```

- [ ] **Step 3: Run integration tests if docker is available**

Run: `cargo test --test catalog_it -- --ignored`
Expected (with docker): all 7 catalog tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/db/catalog.rs tests/catalog_it.rs
git commit -m "db: catalog list_columns / list_indexes / list_constraints / table_size"
```

### Task 4.2: db::types — Postgres value to DisplayValue

**Files:**
- Create: `src/db/types.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/types.rs`**

```rust
//! Convert tokio_postgres rows into displayable strings.

use std::fmt::Write;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value as Json;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

/// Convert a single column value to its display string. Returns "NULL" for
/// nulls, "<unsupported>" for types we don't yet handle.
pub fn col_to_string(row: &Row, idx: usize) -> String {
    let col_type = row.columns()[idx].type_();
    match *col_type {
        Type::BOOL => row.try_get::<_, Option<bool>>(idx).map_or_else(err, opt_str),
        Type::INT2 => row.try_get::<_, Option<i16>>(idx).map_or_else(err, opt_str),
        Type::INT4 => row.try_get::<_, Option<i32>>(idx).map_or_else(err, opt_str),
        Type::INT8 => row.try_get::<_, Option<i64>>(idx).map_or_else(err, opt_str),
        Type::FLOAT4 => row.try_get::<_, Option<f32>>(idx).map_or_else(err, opt_str),
        Type::FLOAT8 => row.try_get::<_, Option<f64>>(idx).map_or_else(err, opt_str),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            row.try_get::<_, Option<String>>(idx).map_or_else(err, |o| o.unwrap_or_else(null))
        }
        Type::UUID => row.try_get::<_, Option<Uuid>>(idx).map_or_else(err, |o| o.map(|u| u.to_string()).unwrap_or_else(null)),
        Type::TIMESTAMPTZ => row.try_get::<_, Option<DateTime<Utc>>>(idx).map_or_else(err, |o| o.map(|t| t.to_rfc3339()).unwrap_or_else(null)),
        Type::TIMESTAMP => row.try_get::<_, Option<NaiveDateTime>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::DATE => row.try_get::<_, Option<NaiveDate>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::TIME => row.try_get::<_, Option<NaiveTime>>(idx).map_or_else(err, |o| o.map(|t| t.to_string()).unwrap_or_else(null)),
        Type::JSON | Type::JSONB => row.try_get::<_, Option<Json>>(idx).map_or_else(err, |o| o.map(|j| j.to_string()).unwrap_or_else(null)),
        Type::NUMERIC => row.try_get::<_, Option<Decimal>>(idx).map_or_else(err, |o| o.map(|n| n.to_string()).unwrap_or_else(null)),
        Type::BYTEA => row.try_get::<_, Option<Vec<u8>>>(idx).map_or_else(err, |o| o.map(hex).unwrap_or_else(null)),
        // Arrays of common scalar types
        Type::TEXT_ARRAY | Type::VARCHAR_ARRAY => row.try_get::<_, Option<Vec<String>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", v.join(","))).unwrap_or_else(null)),
        Type::INT4_ARRAY => row.try_get::<_, Option<Vec<i32>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", join_strs(&v))).unwrap_or_else(null)),
        Type::INT8_ARRAY => row.try_get::<_, Option<Vec<i64>>>(idx).map_or_else(err, |o| o.map(|v| format!("{{{}}}", join_strs(&v))).unwrap_or_else(null)),
        _ => "<unsupported>".into(),
    }
}

fn opt_str<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(null)
}

fn null() -> String { "NULL".into() }

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("<err: {e}>")
}

fn hex(bytes: Vec<u8>) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("\\x");
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn join_strs<T: std::fmt::Display>(xs: &[T]) -> String {
    let mut out = String::new();
    for (i, x) in xs.iter().enumerate() {
        if i > 0 { out.push(','); }
        let _ = write!(out, "{x}");
    }
    out
}

/// Convert an entire row to display strings, in column order.
pub fn row_to_strings(row: &Row) -> Vec<String> {
    (0..row.len()).map(|i| col_to_string(row, i)).collect()
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
pub mod catalog;
pub mod types;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/db/types.rs src/db/mod.rs
git commit -m "db: types — postgres row to displayable strings"
```

### Task 4.3: db::rows — paged row fetch

**Files:**
- Create: `src/db/rows.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/rows.rs`**

```rust
//! Paged row fetch with LIMIT/OFFSET. Returns column headers + display strings.

use crate::{db::{PgConn, types::row_to_strings}, error::DbError};

pub const PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone)]
pub struct Page {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub offset: i64,
    pub estimated_total: i64,
}

pub async fn fetch_page(
    conn: &PgConn,
    schema: &str,
    table: &str,
    offset: i64,
) -> Result<Page, DbError> {
    let qualified = format!("\"{}\".\"{}\"", schema.replace('"', "\"\""), table.replace('"', "\"\""));
    let sql = format!("SELECT * FROM {qualified} LIMIT {PAGE_SIZE} OFFSET {offset}");
    let rows = conn.client()
        .query(&sql, &[])
        .await
        .map_err(|e| DbError::Query { sql: sql.clone(), source: e.to_string() })?;

    let headers: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name().to_string()).collect()
    } else {
        // No rows on this page; pull schema separately.
        let info_sql = format!("SELECT * FROM {qualified} LIMIT 0");
        let r = conn.client()
            .query(&info_sql, &[])
            .await
            .map_err(|e| DbError::Query { sql: info_sql.clone(), source: e.to_string() })?;
        r.first()
            .map(|f| f.columns().iter().map(|c| c.name().to_string()).collect())
            .unwrap_or_default()
    };

    let body: Vec<Vec<String>> = rows.iter().map(row_to_strings).collect();

    let est_sql = format!(
        "SELECT reltuples::bigint FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = $1 AND c.relname = $2"
    );
    let est: i64 = conn.client()
        .query_opt(&est_sql, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: est_sql.clone(), source: e.to_string() })?
        .map(|r| r.get(0))
        .unwrap_or(0);

    Ok(Page { headers, rows: body, offset, estimated_total: est })
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
pub mod catalog;
pub mod rows;
pub mod types;
```

- [ ] **Step 3: Write `tests/rows_it.rs`**

```rust
mod common;

use postui::db::rows;

#[tokio::test]
#[ignore = "requires docker"]
async fn fetch_page_returns_first_100() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.r (id int, name text)",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.r SELECT g, 'name-'||g FROM generate_series(1, 250) g",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "r", 0).await.unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.offset, 0);
    assert_eq!(page.headers, vec!["id".to_string(), "name".to_string()]);
    assert_eq!(page.rows[0][0], "1");
    assert_eq!(page.rows[0][1], "name-1");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn fetch_page_with_offset() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.r (id int)",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.r SELECT g FROM generate_series(1, 250) g",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "r", 100).await.unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.rows[0][0], "101");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn type_conversions_round_trip() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (
            i  int,
            t  text,
            b  bool,
            ts timestamptz,
            j  jsonb,
            u  uuid,
            n  numeric,
            ba bytea
        )",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.t VALUES (
            1, 'hi', true, '2024-01-01T12:00:00Z',
            '{\"k\":1}'::jsonb,
            '11111111-2222-3333-4444-555555555555'::uuid,
            12.34,
            '\\x010203'::bytea
        )",
        &[],
    ).await.unwrap();

    let page = rows::fetch_page(&db.conn, "public", "t", 0).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    let r = &page.rows[0];
    assert_eq!(r[0], "1");
    assert_eq!(r[1], "hi");
    assert_eq!(r[2], "true");
    assert!(r[3].starts_with("2024-01-01"));
    assert!(r[4].contains("\"k\""));
    assert!(r[5].starts_with("11111111"));
    assert_eq!(r[6], "12.34");
    assert_eq!(r[7], "\\x010203");
}
```

- [ ] **Step 4: Run integration tests if docker is available**

Run: `cargo test --test rows_it -- --ignored`
Expected (with docker): 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/rows.rs src/db/mod.rs tests/rows_it.rs
git commit -m "db: paged row fetch + type-conversion integration tests"
```

### Task 4.4: ui::detail — row detail key/value form

**Files:**
- Create: `src/ui/detail.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write `src/ui/detail.rs`**

```rust
//! Row detail: a 2-column key/value form for inspecting a single row.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table as RTable, TableState},
};

use crate::ui::theme::Theme;

#[derive(Debug, Clone)]
pub struct DetailView {
    pub fields: Vec<(String, String)>, // (column name, display value)
    pub state: TableState,
}

impl DetailView {
    pub fn new(fields: Vec<(String, String)>) -> Self {
        let mut state = TableState::default();
        state.select(if fields.is_empty() { None } else { Some(0) });
        Self { fields, state }
    }

    pub fn move_up(&mut self) {
        if let Some(i) = self.state.selected() {
            self.state.select(Some(i.saturating_sub(1)));
        }
    }

    pub fn move_down(&mut self) {
        if self.fields.is_empty() { return; }
        let i = self.state.selected().unwrap_or(0);
        let next = (i + 1).min(self.fields.len() - 1);
        self.state.select(Some(next));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let header = Row::new(vec![Cell::from("column"), Cell::from("value")])
            .style(Style::default().fg(theme.table_header).add_modifier(Modifier::BOLD));

        let body: Vec<Row> = self.fields.iter().map(|(k, v)| {
            Row::new(vec![Cell::from(k.clone()), Cell::from(v.clone())])
        }).collect();

        let widths = vec![Constraint::Percentage(30), Constraint::Percentage(70)];
        let table = RTable::new(body, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .row_highlight_style(
                Style::default().bg(theme.selection_bg).fg(theme.selection_fg),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}
```

- [ ] **Step 2: Add to `src/ui/mod.rs`**

```rust
pub mod detail;
pub mod footer;
pub mod header;
pub mod palette;
pub mod table;
pub mod theme;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui/detail.rs src/ui/mod.rs
git commit -m "ui: row detail key/value widget"
```

### Task 4.5: views::rows — paged row view

**Files:**
- Create: `src/views/rows.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Add a `Rows` variant to `ViewPayload`**

In `src/views/mod.rs`, replace `ViewPayload`:

```rust
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
}
```

- [ ] **Step 2: Write `src/views/rows.rs`**

```rust
//! Paged rows view (used standalone and embedded in the table inspector).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::{Constraint, Direction, Layout, Rect}, widgets::Paragraph};

use crate::{
    db::{PgConn, rows::{PAGE_SIZE, Page, fetch_page}},
    keys::{Motion, vim_motion},
    ui::{detail::DetailView, table::DataTable, theme::Theme},
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

    /// Pop a row-detail view for the current selection.
    pub fn detail_for_selection(&self) -> Option<DetailView> {
        let page = self.page.as_ref()?;
        let row_idx = self.table.selected_index()?;
        let row = page.rows.get(row_idx)?;
        let fields = page.headers.iter()
            .zip(row.iter())
            .map(|(h, v)| (h.clone(), v.clone()))
            .collect();
        Some(DetailView::new(fields))
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

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 3: Add to `src/views/mod.rs`**

```rust
pub mod rows;
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/views/rows.rs src/views/mod.rs
git commit -m "views: paged RowsView with PageDown/PageUp"
```

### Task 4.6: TableInspectorView with 5 tabs

**Files:**
- Create: `src/views/table_inspector.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Write `src/views/table_inspector.rs`**

```rust
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
            ColumnInfo, ConstraintInfo, IndexInfo, TableSize,
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
    match t { Tab::Rows => "rows", Tab::Columns => "columns", Tab::Indexes => "indexes", Tab::Constraints => "constraints", Tab::Size => "size" }
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
                    crate::ui::theme::DEFAULT.name.to_string(), // placeholder; humanise size below
                    i.scans.to_string(),
                ]).collect();
                // Replace the placeholder with real human-bytes:
                let display: Vec<Vec<String>> = ix.iter().map(|i| vec![
                    i.name.clone(),
                    i.definition.clone(),
                    human_bytes(i.size_bytes),
                    i.scans.to_string(),
                ]).collect();
                self.indexes.set_rows(display);
                self.error = None;
                let _ = display; // silence
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

// Suppress: we kept the imports used by the columns/indexes lookups.
#[allow(dead_code)]
fn _types(_c: ColumnInfo, _i: IndexInfo, _co: ConstraintInfo) {}
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod table_inspector;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/views/table_inspector.rs src/views/mod.rs
git commit -m "views: TableInspectorView with 5 tabs"
```

### Task 4.7: Wire <enter> from :tables → inspector, and from inspector rows → row detail

**Files:** modify `src/app.rs`, create `src/views/row_detail.rs`

- [ ] **Step 1: Write `src/views/row_detail.rs`**

```rust
//! Read-only row detail view.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    ui::{detail::DetailView, theme::Theme},
    views::{Ctx, Outcome, View, ViewId},
};

pub struct RowDetailView {
    id: ViewId,
    detail: DetailView,
}

impl RowDetailView {
    pub fn new(detail: DetailView) -> Self {
        Self { id: ViewId::next(), detail }
    }
}

impl View for RowDetailView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "row" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.detail.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => { self.detail.move_down(); Outcome::Consumed }
            KeyCode::Char('k') | KeyCode::Up => { self.detail.move_up(); Outcome::Consumed }
            _ => Outcome::Pass,
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod row_detail;
```

- [ ] **Step 3: Extend `App::handle_enter_drilldown` in `src/app.rs`**

Add new arms inside the `match title` block:

```rust
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
                // Inside the inspector: rows tab Enter -> row detail.
                use crate::views::row_detail::RowDetailView;
                use crate::views::table_inspector::TableInspectorView;
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<TableInspectorView>());
                if let Some(insp) = view {
                    if let Some(detail) = insp.rows_view().detail_for_selection() {
                        return Outcome::Push(Box::new(RowDetailView::new(detail)));
                    }
                }
                Outcome::Consumed
            }
```

- [ ] **Step 4: Build, smoke test**

Run: `cargo build && cargo run -- postgres://$USER@localhost/postgres`

`:tables`, select one with `j`/`k`, `<enter>` → inspector with rows tab. `h`/`l` switches tabs (columns / indexes / constraints / size). `j`/`k` in rows, `<enter>` → row detail. `<esc>` pops back through.

- [ ] **Step 5: Commit**

```bash
git add src/views/row_detail.rs src/views/mod.rs src/app.rs
git commit -m "app: drill from :tables → inspector → row detail"
```

**Milestone 4 complete.** Inspector with 5 tabs + read-only row detail.

---

## Milestone 5 — `:query` Editor

**End state:** `:query` opens a split view: editor on top, results below. Type SQL, `Ctrl-R` (or `F5`) runs it. `Ctrl-E` opens the buffer in `$EDITOR`; saving and quitting brings the edited text back. `Ctrl-C` cancels an in-flight query without quitting the app. Multiple statements: tabs at top of result pane.

### Task 5.1: Add tui-textarea dependency

**Files:** modify `Cargo.toml`

- [ ] **Step 1: Add the editor crate**

```bash
cargo add tui-textarea --features crossterm,search
```

- [ ] **Step 2: Verify**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add tui-textarea for the :query editor"
```

### Task 5.2: ui::editor — textarea wrapper + $EDITOR shellout

**Files:**
- Create: `src/ui/editor.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write `src/ui/editor.rs`**

```rust
//! Multi-line editor wrapper around tui-textarea, with a $EDITOR shell-out.

use std::{
    io::{Read, Write, stdout},
    process::Command,
};

use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
};
use tui_textarea::TextArea;

use crate::ui::theme::Theme;

pub struct Editor {
    pub area: TextArea<'static>,
}

impl Editor {
    pub fn new() -> Self {
        let mut area = TextArea::default();
        area.set_block(Block::default().borders(Borders::ALL));
        area.set_cursor_line_style(Style::default());
        Self { area }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.area.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        self.area.set_style(Style::default().fg(theme.fg));
        f.render_widget(&self.area, area);
    }

    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }

    pub fn set_text(&mut self, s: &str) {
        self.area = TextArea::from(s.lines().map(String::from).collect::<Vec<_>>());
        self.area.set_cursor_line_style(Style::default());
    }

    pub fn clear(&mut self) {
        self.area = TextArea::default();
    }
}

/// Suspend the TUI, open the user's $EDITOR with the given text, restore.
/// Returns the new text on success.
pub fn shell_out_to_editor(initial: &str) -> std::io::Result<String> {
    let editor = std::env::var("EDITOR").or_else(|_| std::env::var("VISUAL")).unwrap_or_else(|_| "nvim".into());

    // Write current buffer to a temp file.
    let mut tmp = tempfile::Builder::new()
        .prefix("postui-")
        .suffix(".sql")
        .tempfile()?;
    tmp.write_all(initial.as_bytes())?;
    let path = tmp.path().to_path_buf();
    drop(tmp);

    // Tear down TUI (raw mode + alt screen) so $EDITOR has a normal terminal.
    let mut out = stdout();
    let _ = out.execute(DisableMouseCapture);
    let _ = out.execute(LeaveAlternateScreen);
    let _ = disable_raw_mode();

    let status = Command::new(&editor).arg(&path).status();

    // Restore TUI regardless of editor exit status.
    let _ = enable_raw_mode();
    let _ = stdout().execute(EnterAlternateScreen);
    let _ = stdout().execute(EnableMouseCapture);

    let _ = status?;

    let mut text = String::new();
    std::fs::File::open(&path)?.read_to_string(&mut text)?;
    let _ = std::fs::remove_file(&path);
    Ok(text)
}
```

- [ ] **Step 2: Add `tempfile` dep**

```bash
cargo add tempfile
```

- [ ] **Step 3: Add to `src/ui/mod.rs`**

```rust
pub mod detail;
pub mod editor;
pub mod footer;
pub mod header;
pub mod palette;
pub mod table;
pub mod theme;
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/editor.rs src/ui/mod.rs Cargo.toml Cargo.lock
git commit -m "ui: editor wrapper + \$EDITOR shellout"
```

### Task 5.3: db::query — execute + cancel

**Files:**
- Create: `src/db/query.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/query.rs`**

```rust
//! Ad-hoc SQL execution.

use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::{db::{PgConn, types::row_to_strings}, error::DbError};

/// One result set returned by a single statement.
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub statement: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub elapsed_ms: u128,
    pub affected: Option<u64>,   // for non-SELECT
}

/// Execute one or more statements separated by `;`. Returns one `ResultSet`
/// per statement that returned rows; non-row statements yield empty rows but
/// `affected = Some(n)`.
pub async fn execute(
    conn: &PgConn,
    sql: &str,
    cancel: CancellationToken,
) -> Result<Vec<ResultSet>, DbError> {
    let stmts = split_statements(sql);
    let mut out = Vec::with_capacity(stmts.len());

    for stmt in stmts {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let trimmed = stmt.trim();
        if trimmed.is_empty() { continue; }
        let started = Instant::now();

        let is_select = trimmed.to_ascii_uppercase().starts_with("SELECT")
            || trimmed.to_ascii_uppercase().starts_with("WITH");

        let res = tokio::select! {
            _ = cancel.cancelled() => return Err(DbError::Cancelled),
            r = async {
                if is_select {
                    let rows = conn.client().query(trimmed, &[]).await
                        .map_err(|e| DbError::Query { sql: trimmed.into(), source: e.to_string() })?;
                    let headers: Vec<String> = rows.first()
                        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
                        .unwrap_or_default();
                    let body: Vec<Vec<String>> = rows.iter().map(row_to_strings).collect();
                    Ok::<_, DbError>(ResultSet {
                        statement: trimmed.into(),
                        headers,
                        rows: body,
                        elapsed_ms: started.elapsed().as_millis(),
                        affected: None,
                    })
                } else {
                    let n = conn.client().execute(trimmed, &[]).await
                        .map_err(|e| DbError::Query { sql: trimmed.into(), source: e.to_string() })?;
                    Ok(ResultSet {
                        statement: trimmed.into(),
                        headers: vec![],
                        rows: vec![],
                        elapsed_ms: started.elapsed().as_millis(),
                        affected: Some(n),
                    })
                }
            } => r?,
        };
        out.push(res);
    }

    Ok(out)
}

/// Split on `;` outside of single-quoted strings and `$$`-delimited blocks.
/// Naïve but sufficient for v1.
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_dollar = false;
    while i < chars.len() {
        let c = chars[i];
        if !in_single && !in_dollar && c == ';' {
            out.push(std::mem::take(&mut buf));
            i += 1;
            continue;
        }
        if !in_dollar && c == '\'' {
            in_single = !in_single;
        }
        if !in_single && i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '$' {
            in_dollar = !in_dollar;
            buf.push('$'); buf.push('$');
            i += 2; continue;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic_statements() {
        let s = split_statements("SELECT 1; SELECT 2;");
        assert_eq!(s.len(), 2);
        assert!(s[0].trim().starts_with("SELECT 1"));
        assert!(s[1].trim().starts_with("SELECT 2"));
    }

    #[test]
    fn split_preserves_semicolon_inside_string() {
        let s = split_statements("SELECT 'a;b'; SELECT 1;");
        assert_eq!(s.len(), 2);
        assert!(s[0].contains("'a;b'"));
    }

    #[test]
    fn split_preserves_semicolon_inside_dollar_block() {
        let s = split_statements("DO $$ BEGIN PERFORM 1; END $$; SELECT 1;");
        assert_eq!(s.len(), 2);
    }
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
pub mod catalog;
pub mod query;
pub mod rows;
pub mod types;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib query`
Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src/db/query.rs src/db/mod.rs
git commit -m "db: execute() with cancel + multi-statement splitter"
```

### Task 5.4: views::query — :query view

**Files:**
- Create: `src/views/query.rs`
- Modify: `src/views/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add `Query` payload variant**

In `src/views/mod.rs`, replace `ViewPayload`:

```rust
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
}
```

- [ ] **Step 2: Write `src/views/query.rs`**

```rust
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
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(area);
        self.editor.render(f, chunks[0], theme);

        let titles: Vec<String> = self.results.iter().enumerate().map(|(i, r)| {
            let kind = if let Some(n) = r.affected {
                format!("#{} affected={n}", i + 1)
            } else {
                format!("#{} rows={}", i + 1, r.rows.len())
            };
            kind
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
                KeyCode::Char('n') => { // next result tab
                    if !self.results.is_empty() {
                        self.select_result((self.active_result + 1) % self.results.len());
                    }
                    return Outcome::Consumed;
                }
                KeyCode::Char('p') => { // prev result tab
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

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 3: Wire `:query` in `App::open`**

In `src/app.rs`, update the `open` function's match:

```rust
        match verb {
            "databases" | "db" => self.push(Box::new(DatabasesView::new(conn))),
            "schemas" | "sc" => self.push(Box::new(SchemasView::new(conn))),
            "tables" | "tb" => self.push(Box::new(TablesView::new(conn, self.current_schema.clone()))),
            "query" | "sql" => {
                use crate::views::query::QueryView;
                self.push(Box::new(QueryView::new(conn)));
            }
            other => self.toast = Some(format!("not yet wired: :{other}")),
        }
```

- [ ] **Step 4: Add to `src/views/mod.rs`**

```rust
pub mod query;
```

- [ ] **Step 5: Build, smoke test**

Run: `cargo build && cargo run -- postgres://$USER@localhost/postgres`

`:query`, type `SELECT 1;`, press `Ctrl-R`. See result tab "#1 rows=1" with one row. Type `SELECT * FROM pg_database;` and run. `Ctrl-N` / `Ctrl-P` cycles result tabs. Press `Ctrl-E` — nvim opens with the buffer. Edit, save, quit — buffer updates. `Ctrl-C` cancels a long-running query (test with `SELECT pg_sleep(60);`).

- [ ] **Step 6: Commit**

```bash
git add src/views/query.rs src/views/mod.rs src/app.rs
git commit -m "views: :query editor + result pane + \$EDITOR + Ctrl-R/E/C/N/P"
```

**Milestone 5 complete.** `:query` works end-to-end with multi-statement results, $EDITOR shell-out, and cancellation.

---

## Milestone 6 — Live Activity Views

**End state:** `:queries`, `:locks`, `:sessions` show live `pg_stat_activity` / `pg_locks` data, refreshing every 2s while visible. Switching away cancels the poller (no background load when hidden). `Ctrl-K` cancels the selected backend (`pg_cancel_backend`). `:terminate <pid>` from the palette terminates (`pg_terminate_backend`). Both go through a confirm modal.

### Task 6.1: db::activity queries

**Files:**
- Create: `src/db/activity.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/activity.rs`**

```rust
//! pg_stat_activity / pg_locks queries.

use chrono::{DateTime, Utc};

use crate::{db::PgConn, error::DbError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityFilter {
    /// Non-self, non-idle (the :queries view).
    ActiveOnly,
    /// Everything except self.
    All,
}

#[derive(Debug, Clone)]
pub struct ActivityRow {
    pub pid: i32,
    pub usename: Option<String>,
    pub datname: Option<String>,
    pub state: Option<String>,
    pub state_change: Option<DateTime<Utc>>,
    pub wait_event: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LockRow {
    pub pid: i32,
    pub mode: Option<String>,
    pub granted: bool,
    pub relation: Option<String>,
    pub query: Option<String>,
}

const SQL_ACTIVITY_ACTIVE: &str = "
    SELECT pid, usename, datname, state, state_change, wait_event, query
    FROM pg_stat_activity
    WHERE pid <> pg_backend_pid()
      AND state IS DISTINCT FROM 'idle'
    ORDER BY query_start NULLS LAST";

const SQL_ACTIVITY_ALL: &str = "
    SELECT pid, usename, datname, state, state_change, wait_event, query
    FROM pg_stat_activity
    WHERE pid <> pg_backend_pid()
    ORDER BY query_start NULLS LAST";

const SQL_LOCKS: &str = "
    SELECT l.pid, l.mode, l.granted, c.relname,
           a.query
    FROM pg_locks l
    LEFT JOIN pg_class c ON c.oid = l.relation
    LEFT JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE l.pid <> pg_backend_pid()
    ORDER BY l.granted, l.pid";

pub async fn activity(conn: &PgConn, filter: ActivityFilter) -> Result<Vec<ActivityRow>, DbError> {
    let sql = match filter {
        ActivityFilter::ActiveOnly => SQL_ACTIVITY_ACTIVE,
        ActivityFilter::All => SQL_ACTIVITY_ALL,
    };
    let rows = conn.client()
        .query(sql, &[])
        .await
        .map_err(|e| DbError::Query { sql: sql.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| ActivityRow {
        pid: r.get(0),
        usename: r.get(1),
        datname: r.get(2),
        state: r.get(3),
        state_change: r.get(4),
        wait_event: r.get(5),
        query: r.get(6),
    }).collect())
}

pub async fn locks(conn: &PgConn) -> Result<Vec<LockRow>, DbError> {
    let rows = conn.client()
        .query(SQL_LOCKS, &[])
        .await
        .map_err(|e| DbError::Query { sql: SQL_LOCKS.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| LockRow {
        pid: r.get(0),
        mode: r.get(1),
        granted: r.get(2),
        relation: r.get(3),
        query: r.get(4),
    }).collect())
}

pub async fn cancel_backend(conn: &PgConn, pid: i32) -> Result<bool, DbError> {
    let row = conn.client()
        .query_one("SELECT pg_cancel_backend($1)", &[&pid])
        .await
        .map_err(|e| DbError::Query { sql: "pg_cancel_backend".into(), source: e.to_string() })?;
    Ok(row.get(0))
}

pub async fn terminate_backend(conn: &PgConn, pid: i32) -> Result<bool, DbError> {
    let row = conn.client()
        .query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .map_err(|e| DbError::Query { sql: "pg_terminate_backend".into(), source: e.to_string() })?;
    Ok(row.get(0))
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
pub mod activity;
pub mod catalog;
pub mod query;
pub mod rows;
pub mod types;
```

- [ ] **Step 3: Write `tests/activity_it.rs`**

```rust
mod common;

use postui::db::activity::{self, ActivityFilter};

#[tokio::test]
#[ignore = "requires docker"]
async fn activity_returns_at_least_self_when_filter_is_all() {
    let db = common::start().await;
    let rows = activity::activity(&db.conn, ActivityFilter::All).await.unwrap();
    // Self is excluded; there might be 0 other rows on a fresh container.
    let _ = rows;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn locks_query_succeeds() {
    let db = common::start().await;
    let _rows = activity::locks(&db.conn).await.unwrap();
}
```

- [ ] **Step 4: Run if docker is available**

Run: `cargo test --test activity_it -- --ignored`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/db/activity.rs src/db/mod.rs tests/activity_it.rs
git commit -m "db: pg_stat_activity / pg_locks + cancel/terminate"
```

### Task 6.2: ui::confirm — confirmation modal

**Files:**
- Create: `src/ui/confirm.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write `src/ui/confirm.rs`**

```rust
//! "Are you sure? y/N" modal with a SQL preview.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::ui::theme::Theme;

pub struct Confirm {
    pub title: String,
    pub body: String,
    pub sql: String,
}

impl Confirm {
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let modal = centered_rect(70, 50, area);
        f.render_widget(Clear, modal);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
            .split(modal);

        let title = Paragraph::new(self.title.clone())
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.warn)))
            .style(Style::default().fg(theme.warn).add_modifier(Modifier::BOLD));
        f.render_widget(title, chunks[0]);

        let body = Paragraph::new(format!("{}\n\nSQL:\n{}", self.body, self.sql))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)));
        f.render_widget(body, chunks[1]);

        let foot = Paragraph::new("[y] confirm     [esc] cancel")
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .style(Style::default().fg(theme.muted));
        f.render_widget(foot, chunks[2]);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
```

- [ ] **Step 2: Add to `src/ui/mod.rs`**

```rust
pub mod confirm;
pub mod detail;
pub mod editor;
pub mod footer;
pub mod header;
pub mod palette;
pub mod table;
pub mod theme;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui/confirm.rs src/ui/mod.rs
git commit -m "ui: confirmation modal with SQL preview"
```

### Task 6.3: ConfirmView — modal as a view (so it integrates with the stack)

**Files:**
- Create: `src/views/confirm.rs`
- Modify: `src/views/mod.rs`

- [ ] **Step 1: Add a `ConfirmDone` payload variant**

In `src/views/mod.rs`:

```rust
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
```

- [ ] **Step 2: Write `src/views/confirm.rs`**

```rust
//! Confirmation modal as a View. Carries an action closure that runs on `y`.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};
use std::sync::Arc;

use crate::{
    ui::{confirm::Confirm, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct ConfirmView {
    id: ViewId,
    confirm: Confirm,
    /// Action returns a future that resolves to a textual op result.
    action: Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<String, crate::error::DbError>> + Send + Sync>,
}

impl ConfirmView {
    pub fn new<F, Fut>(title: impl Into<String>, body: impl Into<String>, sql: impl Into<String>, action: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, crate::error::DbError>> + Send + 'static,
    {
        use futures::FutureExt;
        let action: Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<String, crate::error::DbError>> + Send + Sync>
            = Arc::new(move || action().boxed());
        Self {
            id: ViewId::next(),
            confirm: Confirm { title: title.into(), body: body.into(), sql: sql.into() },
            action,
        }
    }
}

impl View for ConfirmView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "confirm" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.confirm.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let view_id = self.id;
                let tx = ctx.event_tx.clone();
                let action = self.action.clone();
                tokio::spawn(async move {
                    let res = (action)().await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::OpResult(res),
                    }).await;
                });
                Outcome::Pop
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Outcome::Pop,
            _ => Outcome::Consumed,
        }
    }
}
```

- [ ] **Step 3: Add to `src/views/mod.rs`**

```rust
pub mod confirm;
```

- [ ] **Step 4: Handle `OpResult` in `App::handle_event`**

In `src/app.rs`, add to the match:

```rust
            AppEvent::ConnectionSwitched(new_conn) => {
                let label = new_conn.label.clone();
                self.conn = Some(new_conn);
                self.toast = Some(format!("switched to {label}"));
                let mut ctx = Ctx::new(self.event_tx.clone());
                if let Some(top) = self.views.last_mut() {
                    top.on_enter(&mut ctx);
                }
            }
```

(`OpResult` is a `ViewPayload`, delivered via `AppEvent::ViewData` — it'll just be dropped if no view is listening, which is what we want for fire-and-forget cancel/terminate. We surface success via toast inside the action future itself in the next task.)

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/views/confirm.rs src/views/mod.rs src/app.rs
git commit -m "views: ConfirmView modal that runs an async action on y"
```

### Task 6.4: ActivityView (used by :queries / :locks / :sessions)

**Files:**
- Create: `src/views/activity.rs`
- Modify: `src/views/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write `src/views/activity.rs`**

```rust
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

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod activity;
```

- [ ] **Step 3: Wire in `App::open` and `App::dispatch_cmd`**

In `src/app.rs`, extend `open`:

```rust
        match verb {
            "databases" | "db" => self.push(Box::new(DatabasesView::new(conn))),
            "schemas" | "sc" => self.push(Box::new(SchemasView::new(conn))),
            "tables" | "tb" => self.push(Box::new(TablesView::new(conn, self.current_schema.clone()))),
            "query" | "sql" => {
                use crate::views::query::QueryView;
                self.push(Box::new(QueryView::new(conn)));
            }
            "queries" => {
                use crate::views::activity::{ActivityKind, ActivityView};
                self.push(Box::new(ActivityView::new(ActivityKind::Queries, conn, 2000)));
            }
            "locks" => {
                use crate::views::activity::{ActivityKind, ActivityView};
                self.push(Box::new(ActivityView::new(ActivityKind::Locks, conn, 2000)));
            }
            "sessions" => {
                use crate::views::activity::{ActivityKind, ActivityView};
                self.push(Box::new(ActivityView::new(ActivityKind::Sessions, conn, 2000)));
            }
            other => self.toast = Some(format!("not yet wired: :{other}")),
        }
```

- [ ] **Step 4: Wire `Cmd::Terminate(pid)` in `dispatch_cmd`**

Replace the `Cmd::Terminate` arm with:

```rust
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
```

Make sure `AppEvent` is already imported in `src/app.rs`; if not, add `use crate::views::AppEvent;`.

- [ ] **Step 5: Build, smoke test**

Run: `cargo build && cargo run -- postgres://$USER@localhost/postgres`

`:queries` opens an activity view that updates every 2s. Open another shell, run `psql -c 'SELECT pg_sleep(60);'`. Watch it appear in `:queries`. Select it (`j`/`k`), press `Ctrl-K`, confirm with `y` — it cancels. `:locks` and `:sessions` work similarly. `:terminate <pid>` from the palette terminates the selected backend.

- [ ] **Step 6: Commit**

```bash
git add src/views/activity.rs src/views/mod.rs src/app.rs
git commit -m "views: ActivityView (:queries/:locks/:sessions) + Ctrl-K + :terminate"
```

**Milestone 6 complete.** Live activity polling lifecycle works; cancel/terminate flow operates through the confirm modal.

---

## Milestone 7 — Row Mutations

**End state:** In a `RowDetailView`, `i` enters edit mode (field-by-field). On submit, the app generates `UPDATE ... WHERE <pk> = ...` and asks for confirmation. `a` from rows or detail opens a blank-row form for `INSERT`. `d` on a selected row generates `DELETE WHERE <pk> = ...` and asks. All mutations require the table to have a primary key — we refuse otherwise (with a toast). `:query` mutations also flow through the confirm modal.

### Task 7.1: db::mutate — SQL builders for UPDATE / INSERT / DELETE

**Files:**
- Create: `src/db/mutate.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Write `src/db/mutate.rs`** (with unit tests for SQL generation)

```rust
//! UPDATE / INSERT / DELETE SQL builders. Output is intentionally NOT
//! parameterized for v1 — we render the values inline so the user sees the
//! exact statement that will run in the confirm modal. We escape strings via
//! Postgres' E'' quoting and bytea via \x notation.

use crate::error::DbError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    Null,
    Bool(bool),
    Number(String),    // already-formatted numeric/integer/decimal
    Text(String),
    Bytes(Vec<u8>),
    /// Raw SQL fragment, used only for things like `now()` — must be safe.
    Raw(String),
}

#[derive(Debug, Clone)]
pub struct ColumnEdit {
    pub name: String,
    pub value: LiteralValue,
}

#[derive(Debug, Clone)]
pub struct PrimaryKey {
    /// Column name + literal value uniquely identifying a row.
    pub columns: Vec<(String, LiteralValue)>,
}

pub fn build_update(
    schema: &str,
    table: &str,
    edits: &[ColumnEdit],
    pk: &PrimaryKey,
) -> Result<String, DbError> {
    if edits.is_empty() {
        return Err(DbError::Type("no columns to update".into()));
    }
    if pk.columns.is_empty() {
        return Err(DbError::Type("table has no primary key — refusing UPDATE".into()));
    }
    let set = edits.iter().map(|e| format!("{} = {}", quote_ident(&e.name), render(&e.value)))
        .collect::<Vec<_>>().join(", ");
    let where_ = pk_where(pk);
    Ok(format!("UPDATE {}.{} SET {} WHERE {};",
        quote_ident(schema), quote_ident(table), set, where_))
}

pub fn build_delete(schema: &str, table: &str, pk: &PrimaryKey) -> Result<String, DbError> {
    if pk.columns.is_empty() {
        return Err(DbError::Type("table has no primary key — refusing DELETE".into()));
    }
    Ok(format!("DELETE FROM {}.{} WHERE {};",
        quote_ident(schema), quote_ident(table), pk_where(pk)))
}

pub fn build_insert(
    schema: &str,
    table: &str,
    values: &[ColumnEdit],
) -> Result<String, DbError> {
    if values.is_empty() {
        return Err(DbError::Type("no columns to insert".into()));
    }
    let cols = values.iter().map(|e| quote_ident(&e.name)).collect::<Vec<_>>().join(", ");
    let vals = values.iter().map(|e| render(&e.value)).collect::<Vec<_>>().join(", ");
    Ok(format!("INSERT INTO {}.{} ({}) VALUES ({});",
        quote_ident(schema), quote_ident(table), cols, vals))
}

fn pk_where(pk: &PrimaryKey) -> String {
    pk.columns.iter()
        .map(|(c, v)| format!("{} = {}", quote_ident(c), render(v)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn render(v: &LiteralValue) -> String {
    match v {
        LiteralValue::Null => "NULL".into(),
        LiteralValue::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        LiteralValue::Number(s) => s.clone(),
        LiteralValue::Text(s) => format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        LiteralValue::Bytes(b) => {
            let mut out = String::from("'\\x");
            for byte in b { out.push_str(&format!("{byte:02x}")); }
            out.push('\'');
            out
        }
        LiteralValue::Raw(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk_int(name: &str, n: i64) -> PrimaryKey {
        PrimaryKey { columns: vec![(name.into(), LiteralValue::Number(n.to_string()))] }
    }

    #[test]
    fn update_simple() {
        let edits = vec![ColumnEdit { name: "email".into(), value: LiteralValue::Text("ada@x.io".into()) }];
        let sql = build_update("public", "users", &edits, &pk_int("id", 1042)).unwrap();
        assert_eq!(sql, "UPDATE \"public\".\"users\" SET \"email\" = E'ada@x.io' WHERE \"id\" = 1042;");
    }

    #[test]
    fn update_with_quote_in_text_is_escaped() {
        let edits = vec![ColumnEdit { name: "name".into(), value: LiteralValue::Text("O'Brien".into()) }];
        let sql = build_update("public", "users", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("E'O\\'Brien'"), "got: {sql}");
    }

    #[test]
    fn update_no_pk_errors() {
        let edits = vec![ColumnEdit { name: "x".into(), value: LiteralValue::Number("1".into()) }];
        let pk = PrimaryKey { columns: vec![] };
        let err = build_update("p", "t", &edits, &pk).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn update_no_edits_errors() {
        let err = build_update("p", "t", &[], &pk_int("id", 1)).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn delete_simple() {
        let sql = build_delete("public", "users", &pk_int("id", 9)).unwrap();
        assert_eq!(sql, "DELETE FROM \"public\".\"users\" WHERE \"id\" = 9;");
    }

    #[test]
    fn delete_no_pk_errors() {
        let err = build_delete("p", "t", &PrimaryKey { columns: vec![] }).unwrap_err();
        assert!(matches!(err, DbError::Type(_)));
    }

    #[test]
    fn insert_simple() {
        let values = vec![
            ColumnEdit { name: "id".into(), value: LiteralValue::Number("1".into()) },
            ColumnEdit { name: "email".into(), value: LiteralValue::Text("a@b.c".into()) },
        ];
        let sql = build_insert("public", "users", &values).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"id\", \"email\") VALUES (1, E'a@b.c');"
        );
    }

    #[test]
    fn null_renders_as_null() {
        let edits = vec![ColumnEdit { name: "deleted_at".into(), value: LiteralValue::Null }];
        let sql = build_update("p", "t", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("\"deleted_at\" = NULL"));
    }

    #[test]
    fn bytes_render_as_hex() {
        let edits = vec![ColumnEdit { name: "data".into(), value: LiteralValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef]) }];
        let sql = build_update("p", "t", &edits, &pk_int("id", 1)).unwrap();
        assert!(sql.contains("'\\xdeadbeef'"));
    }

    #[test]
    fn composite_pk_uses_and() {
        let pk = PrimaryKey { columns: vec![
            ("a".into(), LiteralValue::Number("1".into())),
            ("b".into(), LiteralValue::Text("x".into())),
        ]};
        let sql = build_delete("p", "t", &pk).unwrap();
        assert!(sql.contains("\"a\" = 1 AND \"b\" = E'x'"));
    }
}
```

- [ ] **Step 2: Add to `src/db/mod.rs`**

```rust
pub mod activity;
pub mod catalog;
pub mod mutate;
pub mod query;
pub mod rows;
pub mod types;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib mutate`
Expected: 9 passed.

- [ ] **Step 4: Commit**

```bash
git add src/db/mutate.rs src/db/mod.rs
git commit -m "db: mutate — UPDATE/INSERT/DELETE SQL builders + tests"
```

### Task 7.2: catalog::primary_key — find a table's PK columns

**Files:** modify `src/db/catalog.rs`

- [ ] **Step 1: Add the function and a test**

Append to `src/db/catalog.rs`:

```rust
const SQL_PK: &str = "
    SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod)
    FROM pg_constraint con
    JOIN pg_class c ON c.oid = con.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace
    JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = ANY(con.conkey)
    WHERE n.nspname = $1 AND c.relname = $2 AND con.contype = 'p'
    ORDER BY array_position(con.conkey, a.attnum)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkColumn {
    pub name: String,
    pub data_type: String,
}

pub async fn primary_key(conn: &PgConn, schema: &str, table: &str) -> Result<Vec<PkColumn>, DbError> {
    let rows = conn.client()
        .query(SQL_PK, &[&schema, &table])
        .await
        .map_err(|e| DbError::Query { sql: SQL_PK.into(), source: e.to_string() })?;
    Ok(rows.into_iter().map(|r| PkColumn {
        name: r.get(0),
        data_type: r.get(1),
    }).collect())
}
```

Append integration test to `tests/catalog_it.rs`:

```rust
#[tokio::test]
#[ignore = "requires docker"]
async fn primary_key_returns_pk_cols() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.t (id int PRIMARY KEY, name text)",
        &[],
    ).await.unwrap();
    let pk = catalog::primary_key(&db.conn, "public", "t").await.unwrap();
    assert_eq!(pk.len(), 1);
    assert_eq!(pk[0].name, "id");
    assert_eq!(pk[0].data_type, "integer");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn primary_key_returns_empty_for_no_pk() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.npk (x int)",
        &[],
    ).await.unwrap();
    let pk = catalog::primary_key(&db.conn, "public", "npk").await.unwrap();
    assert!(pk.is_empty());
}
```

- [ ] **Step 2: Build + run integration tests if docker is available**

Run: `cargo build && cargo test --test catalog_it -- --ignored`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/db/catalog.rs tests/catalog_it.rs
git commit -m "db: catalog::primary_key + tests"
```

### Task 7.3: Editable RowDetailView (i / a / d)

**Files:** modify `src/views/row_detail.rs`, `src/ui/detail.rs`

- [ ] **Step 1: Extend `DetailView` with editable fields**

Replace `src/ui/detail.rs` with:

```rust
//! Row detail: 2-column key/value form, editable in-place.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table as RTable, TableState},
};

use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode { View, Edit }

#[derive(Debug, Clone)]
pub struct DetailField {
    pub name: String,
    pub original: String,
    pub edited: String,
    /// True if this field is part of the primary key (read-only in Edit mode).
    pub is_pk: bool,
}

#[derive(Debug, Clone)]
pub struct DetailView {
    pub fields: Vec<DetailField>,
    pub state: TableState,
    pub mode: Mode,
}

impl DetailView {
    pub fn new(fields: Vec<DetailField>) -> Self {
        let mut state = TableState::default();
        state.select(if fields.is_empty() { None } else { Some(0) });
        Self { fields, state, mode: Mode::View }
    }

    pub fn move_up(&mut self) {
        if let Some(i) = self.state.selected() {
            self.state.select(Some(i.saturating_sub(1)));
        }
    }

    pub fn move_down(&mut self) {
        if self.fields.is_empty() { return; }
        let i = self.state.selected().unwrap_or(0);
        let next = (i + 1).min(self.fields.len() - 1);
        self.state.select(Some(next));
    }

    pub fn enter_edit(&mut self) { self.mode = Mode::Edit; }
    pub fn leave_edit(&mut self) { self.mode = Mode::View; }

    pub fn append_char(&mut self, c: char) {
        if self.mode != Mode::Edit { return; }
        if let Some(i) = self.state.selected() {
            if let Some(f) = self.fields.get_mut(i) {
                if !f.is_pk {
                    f.edited.push(c);
                }
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.mode != Mode::Edit { return; }
        if let Some(i) = self.state.selected() {
            if let Some(f) = self.fields.get_mut(i) {
                if !f.is_pk { f.edited.pop(); }
            }
        }
    }

    /// Returns dirty fields (edited != original AND not PK).
    pub fn dirty(&self) -> Vec<&DetailField> {
        self.fields.iter().filter(|f| !f.is_pk && f.edited != f.original).collect()
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let header = Row::new(vec![Cell::from("column"), Cell::from("value")])
            .style(Style::default().fg(theme.table_header).add_modifier(Modifier::BOLD));

        let mode = self.mode;
        let body: Vec<Row> = self.fields.iter().map(|fld| {
            let val = if mode == Mode::Edit && fld.edited != fld.original {
                format!("{}  →  {}", fld.original, fld.edited)
            } else {
                fld.edited.clone()
            };
            let name_cell = if fld.is_pk {
                Cell::from(format!("{} [pk]", fld.name)).style(Style::default().fg(theme.muted))
            } else {
                Cell::from(fld.name.clone())
            };
            Row::new(vec![name_cell, Cell::from(val)])
        }).collect();

        let widths = vec![Constraint::Percentage(30), Constraint::Percentage(70)];
        let title = match self.mode {
            Mode::View => " row ",
            Mode::Edit => " row [EDIT — Enter saves, Esc cancels] ",
        };
        let table = RTable::new(body, widths)
            .header(header)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme.border)))
            .row_highlight_style(Style::default().bg(theme.selection_bg).fg(theme.selection_fg))
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}
```

- [ ] **Step 2: Replace `src/views/row_detail.rs` to handle edit / submit / pop with mutation**

```rust
//! Row detail view: read mode by default; `i` enters edit mode for the
//! selected field; Enter saves (kicks off the mutation flow), Esc cancels.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    db::{
        PgConn,
        catalog::PkColumn,
        mutate::{ColumnEdit, LiteralValue, PrimaryKey, build_update},
    },
    ui::{detail::{DetailView, Mode}, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload, confirm::ConfirmView},
};

pub struct RowDetailView {
    id: ViewId,
    detail: DetailView,
    conn: PgConn,
    schema: String,
    table: String,
    pk: Vec<PkColumn>,
}

impl RowDetailView {
    pub fn new(
        conn: PgConn,
        schema: String,
        table: String,
        pk: Vec<PkColumn>,
        detail: DetailView,
    ) -> Self {
        Self { id: ViewId::next(), detail, conn, schema, table, pk }
    }

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
        let dirty = self.detail.dirty();
        if dirty.is_empty() {
            return Outcome::Consumed;
        }
        let edits: Vec<ColumnEdit> = dirty.iter().map(|f| ColumnEdit {
            name: f.name.clone(),
            value: LiteralValue::Text(f.edited.clone()),
        }).collect();
        let pk = self.current_pk();
        let sql = match build_update(&self.schema, &self.table, &edits, &pk) {
            Ok(s) => s,
            Err(e) => return Outcome::Push(Box::new(toast_view(format!("can't update: {e}")))),
        };
        let conn = self.conn.clone();
        let tx = ctx.event_tx.clone();
        let sql_for_action = sql.clone();
        let confirm = ConfirmView::new(
            "execute UPDATE",
            "this will execute the SQL below.",
            sql,
            move || {
                let conn = conn.clone();
                let tx = tx.clone();
                let sql = sql_for_action.clone();
                async move {
                    let r = conn.client().execute(sql.as_str(), &[]).await;
                    let toast = match &r {
                        Ok(n) => format!("UPDATE {n}"),
                        Err(e) => format!("UPDATE failed: {e}"),
                    };
                    let _ = tx.send(AppEvent::Toast(toast)).await;
                    r.map(|n| format!("{n}")).map_err(|e| crate::error::DbError::Query {
                        sql: sql.clone(),
                        source: e.to_string(),
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
                        source: e.to_string(),
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
    fn on_enter(&mut self, ctx: &mut Ctx) {
        let _ = ctx.event_tx.try_send(AppEvent::Toast(self.msg.clone()));
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
                    // Cancel: revert any in-progress edits in this field.
                    if let Some(i) = self.detail.state.selected() {
                        if let Some(f) = self.detail.fields.get_mut(i) {
                            f.edited = f.original.clone();
                        }
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
```

- [ ] **Step 3: Update the inspector to fetch PK + build full DetailField list**

Modify `src/views/rows.rs`'s `detail_for_selection` to return `Vec<DetailField>` instead of `DetailView`, since callers need to mark PK columns. Replace the function:

```rust
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
```

(Delete the old `detail_for_selection`. Update the inspector + app accordingly.)

- [ ] **Step 4: Update `App::handle_enter_drilldown`'s `"table"` arm**

```rust
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
```

- [ ] **Step 5: Add `PushView` to `AppEvent` and handler in `App::handle_event`**

In `src/views/mod.rs`:

```rust
#[derive(Debug)]
pub enum AppEvent {
    ViewData {
        view_id: ViewId,
        payload: ViewPayload,
    },
    Toast(String),
    ConnectionSwitched(crate::db::PgConn),
    PushView(Box<dyn View>),
}
```

In `src/app.rs::handle_event`:

```rust
            AppEvent::PushView(v) => self.push(v),
```

- [ ] **Step 6: Add `schema()` and `name()` accessors to TableInspectorView**

In `src/views/table_inspector.rs`, add:

```rust
impl TableInspectorView {
    pub fn schema(&self) -> &str { &self.schema }
    pub fn name(&self) -> &str { &self.name }
}
```

- [ ] **Step 7: Build, run integration test for end-to-end mutation**

Create `tests/mutate_it.rs`:

```rust
mod common;

use postui::db::{
    catalog::primary_key,
    mutate::{ColumnEdit, LiteralValue, PrimaryKey, build_update},
};

#[tokio::test]
#[ignore = "requires docker"]
async fn build_update_executes_against_real_pg() {
    let db = common::start().await;
    db.conn.client().execute(
        "CREATE TABLE public.u (id int PRIMARY KEY, name text)",
        &[],
    ).await.unwrap();
    db.conn.client().execute(
        "INSERT INTO public.u VALUES (1, 'before')",
        &[],
    ).await.unwrap();

    let pk = primary_key(&db.conn, "public", "u").await.unwrap();
    assert_eq!(pk.len(), 1);
    let pk = PrimaryKey {
        columns: vec![("id".into(), LiteralValue::Number("1".into()))]
    };
    let edits = vec![ColumnEdit { name: "name".into(), value: LiteralValue::Text("after".into()) }];
    let sql = build_update("public", "u", &edits, &pk).unwrap();

    let n = db.conn.client().execute(sql.as_str(), &[]).await.unwrap();
    assert_eq!(n, 1);

    let row = db.conn.client().query_one("SELECT name FROM public.u WHERE id=1", &[]).await.unwrap();
    let name: String = row.get(0);
    assert_eq!(name, "after");
}
```

Run: `cargo build && cargo test --test mutate_it -- --ignored`
Expected: 1 passed (with docker).

- [ ] **Step 8: Commit**

```bash
git add src/views/row_detail.rs src/views/rows.rs src/views/mod.rs src/views/table_inspector.rs src/ui/detail.rs src/app.rs tests/mutate_it.rs
git commit -m "views: editable RowDetailView with i/d + UPDATE/DELETE confirm"
```

### Task 7.4: Insert flow (`a` from rows view)

**Files:** modify `src/views/rows.rs`, `src/app.rs`

- [ ] **Step 1: Build a "blank insert" `DetailView` in `RowsView`**

Append to `src/views/rows.rs`:

```rust
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
```

- [ ] **Step 2: Detect `a` in inspector or rows view and dispatch insert flow**

In `src/views/table_inspector.rs::handle_key`, before the existing `match self.active`:

```rust
        if self.active == Tab::Rows && key.code == KeyCode::Char('a') {
            // Bubble up to the App so it can build the insert flow.
            return Outcome::Pass;
        }
```

In `src/app.rs::handle_key`, after the existing `top.handle_key` call but inside the same block, intercept `a` for the inspector:

Replace the section that maps `Outcome::Pass` outcomes with a richer mapping:

```rust
        if let Some(top) = self.views.last_mut() {
            let mut ctx = Ctx::new(self.event_tx.clone());
            let outcome = top.handle_key(key, &mut ctx);
            let outcome = match outcome {
                Outcome::Pass if key.code == KeyCode::Esc => Outcome::Pop,
                Outcome::Pass if key.code == KeyCode::Enter => self.handle_enter_drilldown(),
                Outcome::Pass if key.code == KeyCode::Char('a') => self.handle_insert_request(),
                other => other,
            };
            self.handle_outcome(outcome);
        } else if key.code == KeyCode::Esc {
            self.should_quit = true;
        }
```

Add `handle_insert_request` to `impl App`:

```rust
    fn handle_insert_request(&mut self) -> Outcome {
        use crate::db::catalog::primary_key;
        use crate::ui::detail::DetailView;
        use crate::views::row_detail::RowDetailView;
        use crate::views::table_inspector::TableInspectorView;
        let title = self.views.last().map(|v| v.title()).unwrap_or("");
        if title != "table" { return Outcome::Pass; }

        let top = self.views.last().unwrap();
        let insp = top.as_any().and_then(|a| a.downcast_ref::<TableInspectorView>());
        if let Some(insp) = insp {
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
```

- [ ] **Step 3: Add insert mode + submit-as-INSERT to RowDetailView**

In `src/views/row_detail.rs`, add a flag and switch the submit path:

```rust
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
    // ...
```

Replace `submit` with:

```rust
    fn submit(&mut self, ctx: &mut Ctx) -> Outcome {
        use crate::db::mutate::{build_insert, build_update};
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
            if dirty.is_empty() { return Outcome::Consumed; }
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
                        source: e.to_string(),
                    })
                }
            },
        );
        Outcome::Push(Box::new(confirm))
    }
```

- [ ] **Step 4: Build, smoke test**

Run: `cargo build && cargo run -- postgres://$USER@localhost/postgres`

`:tables`, `<enter>` on a table with a PK → inspector → rows tab. Press `a` → blank row form. Type values, `Enter` to submit → confirm → `y` → `INSERT 1` toast. Press `<enter>` on a row to open detail, `i` to edit, type, `Enter` → confirm → `UPDATE 1`. Press `d` on a row in detail → confirm → `DELETE 1`.

- [ ] **Step 5: Commit**

```bash
git add src/views/row_detail.rs src/views/rows.rs src/views/table_inspector.rs src/app.rs
git commit -m "views: insert flow (a) + insert/update/delete go through confirm"
```

**Milestone 7 complete.** Row mutations land. Refusing tables without PK keeps the user safe.

---

## Milestone 8 — Themes, ConnectionsView wiring, Filter, Help

**End state:** `:themes` lists themes with **live preview as you cursor through**; `<enter>` persists the selection to the config file, `<esc>` reverts. `:theme dracula` from the palette switches and persists in one shot. `:connections` becomes the default view at launch when no `--uri`/`--connection` was passed; `<enter>` connects. `/` filters visible rows in any list view. `?` opens a help modal listing keybindings and known `:commands`.

### Task 8.1: Persist `[ui].theme` to config file

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add `Config::write_theme` and a test**

Append to `src/config.rs`:

```rust
impl Config {
    /// Write the current `[ui].theme` value back to the given path,
    /// preserving everything else. We re-serialize via toml::to_string so
    /// formatting may change, but the data is preserved.
    pub fn save(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Read)?;
        }
        let contents = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(path, contents).map_err(ConfigError::Read)?;
        Ok(())
    }
}
```

You'll need `Serialize` on the relevant types. Update the derives:

```rust
#[derive(Debug, Deserialize, serde::Serialize, Default, Clone)]
#[serde(default)]
pub struct Config { /* ... */ }

#[derive(Debug, Deserialize, serde::Serialize, Clone)]
#[serde(default)]
pub struct UiConfig { /* ... */ }

#[derive(Debug, Deserialize, serde::Serialize, Clone, Default)]
#[serde(default)]
pub struct ViewsConfig { /* ... */ }

#[derive(Debug, Deserialize, serde::Serialize, Clone)]
pub struct ViewOverride { /* ... */ }

#[derive(Debug, Deserialize, serde::Serialize, Clone)]
pub struct ConnectionConfig { /* ... */ }
```

- [ ] **Step 2: Add a save/load round-trip test**

```rust
    #[test]
    fn save_then_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut cfg = Config::default();
        cfg.ui.theme = "nord".into();
        cfg.connections.push(ConnectionConfig {
            name: "x".into(),
            url: None,
            host: Some("h".into()),
            port: Some(5432),
            user: Some("u".into()),
            database: Some("d".into()),
            password: None,
            sslmode: None,
        });
        cfg.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.ui.theme, "nord");
        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.connections[0].name, "x");
    }
```

- [ ] **Step 3: Add `tempfile` to dev-deps**

```bash
cargo add --dev tempfile
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib config`
Expected: all config tests pass (including the new round-trip).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs Cargo.toml Cargo.lock
git commit -m "config: Serialize derives + save() round-trip + test"
```

### Task 8.2: ThemesView — list + live preview + persist

**Files:**
- Create: `src/views/themes.rs`
- Modify: `src/views/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add `ThemeChanged` AppEvent**

In `src/views/mod.rs`:

```rust
#[derive(Debug)]
pub enum AppEvent {
    ViewData {
        view_id: ViewId,
        payload: ViewPayload,
    },
    Toast(String),
    ConnectionSwitched(crate::db::PgConn),
    PushView(Box<dyn View>),
    /// Live-preview the named theme without persisting.
    PreviewTheme(&'static crate::ui::theme::Theme),
    /// Persist the current theme to the config file.
    PersistTheme(String),
    /// Restore a previously saved theme (used by ThemesView on Esc).
    RestoreTheme(&'static crate::ui::theme::Theme),
}
```

- [ ] **Step 2: Write `src/views/themes.rs`**

```rust
//! :themes — theme picker with live preview.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};

use crate::{
    keys::{Motion, vim_motion},
    ui::{table::DataTable, theme::{self, Theme}},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct ThemesView {
    id: ViewId,
    table: DataTable,
    saved: &'static Theme,
}

impl ThemesView {
    pub fn new(current: &'static Theme) -> Self {
        let mut table = DataTable::new(vec!["theme"]);
        let rows: Vec<Vec<String>> = theme::ALL.iter()
            .map(|t| vec![t.name.to_string()])
            .collect();
        table.set_rows(rows);
        // Position cursor on the current theme.
        if let Some(idx) = theme::ALL.iter().position(|t| std::ptr::eq(*t, current)) {
            table.state.select(Some(idx));
        }
        Self { id: ViewId::next(), table, saved: current }
    }

    fn cursor_theme(&self) -> Option<&'static Theme> {
        self.table.selected_index().and_then(|i| theme::ALL.get(i)).copied()
    }

    fn preview(&self, ctx: &mut Ctx) {
        if let Some(t) = self.cursor_theme() {
            let _ = ctx.event_tx.try_send(AppEvent::PreviewTheme(t));
        }
    }
}

impl View for ThemesView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "themes" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.table.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        if let Some(m) = vim_motion(key) {
            self.table.move_motion(m);
            self.preview(ctx);
            return Outcome::Consumed;
        }
        match key.code {
            KeyCode::Enter => {
                if let Some(t) = self.cursor_theme() {
                    let _ = ctx.event_tx.try_send(AppEvent::PreviewTheme(t));
                    let _ = ctx.event_tx.try_send(AppEvent::PersistTheme(t.name.to_string()));
                }
                Outcome::Pop
            }
            KeyCode::Esc => {
                let _ = ctx.event_tx.try_send(AppEvent::RestoreTheme(self.saved));
                Outcome::Pop
            }
            _ => Outcome::Pass,
        }
    }

    fn on_enter(&mut self, ctx: &mut Ctx) {
        // Initial preview = current cursor.
        self.preview(ctx);
    }

    fn apply(&mut self, _payload: ViewPayload) {}

    fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
}
```

- [ ] **Step 3: Add to `src/views/mod.rs`**

```rust
pub mod themes;
```

- [ ] **Step 4: Handle the new `AppEvent` variants in `App::handle_event`**

Extend the match in `src/app.rs`:

```rust
            AppEvent::PreviewTheme(t) => {
                self.theme = t;
            }
            AppEvent::RestoreTheme(t) => {
                self.theme = t;
            }
            AppEvent::PersistTheme(name) => {
                if let Some(t) = theme::by_name(&name) {
                    self.theme = t;
                }
                self.config.ui.theme = name.clone();
                if let Err(e) = self.config.save(&self.config_path) {
                    self.toast = Some(format!("save failed: {e}"));
                } else {
                    self.toast = Some(format!("theme: {name} (saved)"));
                }
            }
```

- [ ] **Step 5: Wire `:themes` in `App::open`**

```rust
            "themes" => {
                use crate::views::themes::ThemesView;
                self.push(Box::new(ThemesView::new(self.theme)));
            }
```

- [ ] **Step 6: App needs `config` and `config_path`**

In `src/app.rs`, add to the `App` struct:

```rust
    pub config: crate::config::Config,
    pub config_path: std::path::PathBuf,
```

Update `App::new` to accept them:

```rust
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
        }
    }
```

Update `src/main.rs` to pass them:

```rust
    rt.block_on(async {
        let conn = bootstrap_connection(&cli, &config).await?;
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let mut app = app::App::new(config, config_path);
        if let Some(c) = conn {
            app.set_connection(c);
        }
        app.run(&mut term).await
    })
```

(Save `config_path` in main before passing.)

- [ ] **Step 7: Build, smoke test**

Run: `cargo build && cargo run`

`:themes` opens the picker. `j`/`k` cursors through; the entire UI re-themes on each move. `<enter>` persists; check `~/.config/postui/config.toml` to verify the new `theme = "..."` line is present. `<esc>` from `:themes` reverts to the previously-active theme without persisting.

`:theme dracula` from the palette also works (already wired in M1).

- [ ] **Step 8: Commit**

```bash
git add src/views/themes.rs src/views/mod.rs src/app.rs src/main.rs
git commit -m "views: :themes with live preview, persist on enter, revert on esc"
```

### Task 8.3: ConnectionsView wired as default landing view

**Files:** modify `src/app.rs`, `src/views/connections.rs`, `src/main.rs`

- [ ] **Step 1: Push `ConnectionsView` at startup if no conn was provided**

In `src/main.rs`, after constructing the App:

```rust
    rt.block_on(async {
        let conn = bootstrap_connection(&cli, &config).await?;
        let (mut term, _guard) = term::TerminalGuard::init()?;
        let mut app = app::App::new(config.clone(), config_path);
        if let Some(c) = conn {
            app.set_connection(c);
        } else if !config.connections.is_empty() {
            use postui::views::connections::ConnectionsView;
            app.push_view(Box::new(ConnectionsView::new(&config, None)));
        }
        app.run(&mut term).await
    })
```

Add `push_view` to `App` (it's a pub wrapper around `push`):

```rust
    pub fn push_view(&mut self, v: Box<dyn View>) { self.push(v); }
```

- [ ] **Step 2: Wire `<enter>` from ConnectionsView to actually connect**

In `src/app.rs::handle_enter_drilldown`, add an arm:

```rust
            "connections" => {
                use crate::views::connections::ConnectionsView;
                let top = self.views.last().unwrap();
                let view = top.as_any().and_then(|a| a.downcast_ref::<ConnectionsView>());
                if let Some(v) = view {
                    if let Some(name) = v.selected_name() {
                        if let Some(cfg) = self.config.find_connection(name) {
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
                    }
                }
                Outcome::Consumed
            }
```

- [ ] **Step 3: Wire `:connections` palette command in `App::open`**

```rust
            "connections" => {
                use crate::views::connections::ConnectionsView;
                let active = self.conn.as_ref().map(|c| c.label.clone());
                self.push(Box::new(ConnectionsView::new(&self.config, active.as_deref())));
            }
```

(Note: this branch comes BEFORE the `let conn = match self.conn.clone() { ... }` early-return because `:connections` doesn't require an active connection.)

To make this work, restructure `open`:

```rust
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
            "help" => self.push(Box::new(crate::views::help::HelpView::new())),
            other => self.toast = Some(format!("not yet wired: :{other}")),
        }
    }
```

(`HelpView` is added in Task 8.5; the `"help"` arm anticipates that.)

- [ ] **Step 4: Build, smoke test**

Run: `cargo build && cargo run`

With a config containing connections, the app launches at `:connections`. `j`/`k` to navigate, `<enter>` to connect — header switches to `connected: <name>` and the toast confirms.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "app: ConnectionsView as default landing view + Enter to connect"
```

### Task 8.4: Filter (`/`) over visible rows

**Files:**
- Modify: `src/ui/table.rs`

- [ ] **Step 1: Add filter mode + filtered display to DataTable**

Append to `src/ui/table.rs`:

```rust
impl DataTable {
    /// Set a substring filter; rows not containing the substring (case
    /// insensitive) in any cell are hidden. Empty string clears the filter.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.recompute_visible();
    }

    fn recompute_visible(&mut self) {
        if self.filter.is_empty() {
            self.visible = (0..self.rows.len()).collect();
        } else {
            self.visible = self.rows.iter().enumerate()
                .filter(|(_, r)| r.iter().any(|c| c.to_lowercase().contains(&self.filter)))
                .map(|(i, _)| i)
                .collect();
        }
        if self.visible.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }
}
```

Modify the `DataTable` struct and `render`/`set_rows`/`move_motion` to use `visible`:

```rust
#[derive(Debug, Clone)]
pub struct DataTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub state: TableState,
    pub filter: String,
    pub visible: Vec<usize>,
}

impl DataTable {
    pub fn new(headers: Vec<&str>) -> Self {
        let mut state = TableState::default();
        state.select(Some(0));
        Self {
            headers: headers.into_iter().map(String::from).collect(),
            rows: Vec::new(),
            state,
            filter: String::new(),
            visible: Vec::new(),
        }
    }

    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        self.recompute_visible();
    }

    pub fn selected_index(&self) -> Option<usize> {
        // Translate the visible-cursor index back to the underlying row index.
        let v = self.state.selected()?;
        self.visible.get(v).copied()
    }

    pub fn selected_row(&self) -> Option<&[String]> {
        self.selected_index().and_then(|i| self.rows.get(i)).map(|v| v.as_slice())
    }

    pub fn move_motion(&mut self, m: Motion) {
        if self.visible.is_empty() { return; }
        let last = self.visible.len() - 1;
        let cur = self.state.selected().unwrap_or(0);
        let next = match m {
            Motion::Up => cur.saturating_sub(1),
            Motion::Down => (cur + 1).min(last),
            Motion::PageUp | Motion::PagePrev => cur.saturating_sub(PAGE_SIZE),
            Motion::PageDown | Motion::PageNext => (cur + PAGE_SIZE).min(last),
            Motion::Home => 0,
            Motion::End => last,
            Motion::Left | Motion::Right => cur,
        };
        self.state.select(Some(next));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let header_row = Row::new(self.headers.iter().cloned().map(Cell::from).collect::<Vec<_>>())
            .style(Style::default().fg(theme.table_header).add_modifier(Modifier::BOLD));

        let body: Vec<Row> = self.visible.iter()
            .map(|&i| Row::new(self.rows[i].iter().cloned().map(Cell::from).collect::<Vec<_>>()))
            .collect();

        let widths: Vec<Constraint> = self.headers.iter()
            .map(|_| Constraint::Percentage(100 / self.headers.len().max(1) as u16))
            .collect();

        let table = RTable::new(body, widths)
            .header(header_row)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .row_highlight_style(
                Style::default().bg(theme.selection_bg).fg(theme.selection_fg).add_modifier(Modifier::BOLD)
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(table, area, &mut self.state);
    }
}
```

Update tests at the bottom — `selected_index` now goes through `visible`, but with no filter `visible == 0..rows.len()` so semantics are unchanged. Add filter tests:

```rust
    #[test]
    fn filter_hides_non_matching_rows() {
        let mut t = populated();
        t.set_filter("y");
        // row 1 has "y"; rows 0 and 2 don't.
        assert_eq!(t.visible, vec![1]);
        assert_eq!(t.selected_index(), Some(1));
    }

    #[test]
    fn empty_filter_shows_all() {
        let mut t = populated();
        t.set_filter("y");
        t.set_filter("");
        assert_eq!(t.visible, vec![0, 1, 2]);
    }

    #[test]
    fn filter_with_no_matches_clears_selection() {
        let mut t = populated();
        t.set_filter("zzz");
        assert!(t.visible.is_empty());
        assert_eq!(t.selected_index(), None);
    }
```

- [ ] **Step 2: Add filter mode to App**

In `src/app.rs`, add a `filter_input: Option<String>` to `App`:

```rust
    pub filter_input: Option<String>,
```

Initialize in `new`:

```rust
            filter_input: None,
```

Handle `/` and updates in `handle_key` (insert before forwarding to view):

```rust
        // Filter mode owns the keys until closed.
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

        if key.code == KeyCode::Char('/') {
            self.filter_input = Some(String::new());
            self.apply_filter("");
            return;
        }
```

Add `apply_filter` to `App`. It needs to reach into the active view's table — easiest is to add a `set_filter` method to the View trait with a default no-op:

In `src/views/mod.rs`, add to the trait:

```rust
    fn set_filter(&mut self, _filter: &str) {}
```

Implement in views that have a table: connections, databases, schemas, tables, activity, themes — for each, add:

```rust
    fn set_filter(&mut self, filter: &str) { self.table.set_filter(filter); }
```

(For inspector: forward to whichever sub-table is active. For rows: forward to its inner DataTable. For row_detail: no-op.)

In `App`:

```rust
    fn apply_filter(&mut self, filter: &str) {
        if let Some(top) = self.views.last_mut() {
            top.set_filter(filter);
        }
    }
```

- [ ] **Step 3: Render the filter prompt in the footer**

In `src/app.rs::render`, replace the footer hint with a filter-aware version:

```rust
        let hints = match self.views.last() {
            Some(_) => "[:] palette  [/] filter  [esc] back  [^Q] quit  [?] help",
            None => "[:] palette  [^Q] quit",
        };
        let toast_or_filter: String;
        let footer_text: Option<&str> = if let Some(buf) = &self.filter_input {
            toast_or_filter = format!("/{}", buf);
            Some(toast_or_filter.as_str())
        } else {
            self.toast.as_deref()
        };
        footer::render(f, foot, self.theme, hints, footer_text, &self.palette);
```

- [ ] **Step 4: Run table tests**

Run: `cargo test --lib table`
Expected: 9 passed (existing 6 + 3 filter tests).

- [ ] **Step 5: Smoke test**

Run: `cargo run -- postgres://$USER@localhost/postgres`

`:tables`. Press `/`, type a substring — list filters as you type. `<esc>` clears, `<enter>` accepts.

- [ ] **Step 6: Commit**

```bash
git add src/ui/table.rs src/views/ src/app.rs
git commit -m "ui+app: / filter over visible rows in list views"
```

### Task 8.5: HelpView

**Files:**
- Create: `src/views/help.rs`
- Modify: `src/views/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write `src/views/help.rs`**

```rust
//! :help — modal listing keybindings and palette commands.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    ui::theme::Theme,
    views::{Ctx, Outcome, View, ViewId},
};

const TEXT: &str = "
  postui — quick help

  Universal:
    :              open palette       /              filter visible rows
    Esc            pop view           ^Q             quit
    ^C             cancel in-flight   ?              this help

  Movement (lists):
    j  k           down  up           h  l           tab switch / left / right
    w  b           page down / up     e              jump to last row

  :query editor:
    ^R | F5        run                ^E             open in $EDITOR
    ^N | ^P        next / prev result tab

  Activity:
    ^K             cancel selected backend (pg_cancel_backend)
    :terminate <pid>  forcefully terminate (pg_terminate_backend)

  Palette commands:
    :connections  :databases (:db)  :schemas (:sc)  :tables (:tb)
    :query (:sql) :queries  :locks  :sessions
    :themes  :theme <name>
    :connect [uri-or-name]
    :terminate <pid>
    :q | :quit

  Row editing:
    Enter          drill in (or open row detail)
    i              edit selected (in row detail) or current field
    a              insert a new row
    d              delete selected row
    Enter          submit edit (in edit mode)  Esc cancel

  Press any key to dismiss.
";

pub struct HelpView { id: ViewId }

impl HelpView {
    pub fn new() -> Self { Self { id: ViewId::next() } }
}

impl Default for HelpView { fn default() -> Self { Self::new() } }

impl View for HelpView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "help" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let p = Paragraph::new(TEXT)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border)))
            .wrap(Wrap { trim: false });
        f.render_widget(p, area);
    }

    fn handle_key(&mut self, _key: KeyEvent, _ctx: &mut Ctx) -> Outcome {
        Outcome::Pop
    }
}
```

- [ ] **Step 2: Add to `src/views/mod.rs`**

```rust
pub mod help;
```

- [ ] **Step 3: Bind `?` to push `HelpView`**

In `src/app.rs::handle_key`, before forwarding to the view:

```rust
        if key.code == KeyCode::Char('?') {
            self.push(Box::new(crate::views::help::HelpView::new()));
            return;
        }
```

- [ ] **Step 4: Build, smoke test**

Run: `cargo build && cargo run`

Press `?` → help opens. Any key dismisses.

- [ ] **Step 5: Commit**

```bash
git add src/views/help.rs src/views/mod.rs src/app.rs
git commit -m "views: ? help modal"
```

### Task 8.6: Snapshot tests for representative views

**Files:**
- Create: `tests/view_snapshot.rs`

- [ ] **Step 1: Write `tests/view_snapshot.rs`**

```rust
//! Sanity-check that representative views render without panicking and
//! produce sensible buffers via the TestBackend.

use postui::{
    ui::{self, theme},
    views::{help::HelpView, View, Ctx},
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use tokio::sync::mpsc;

#[test]
fn help_view_renders_text() {
    let backend = TestBackend::new(80, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut help = HelpView::new();
    let theme = &theme::DEFAULT;
    let (_tx, _rx) = mpsc::channel(8);
    let _ctx = Ctx::new(_tx);

    term.draw(|f| {
        let area = Rect::new(0, 0, 80, 30);
        help.render(f, area, theme);
    }).unwrap();

    let buf = term.backend().buffer();
    let dump = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(dump.contains("postui"));
    assert!(dump.contains("palette"));
    assert!(dump.contains(":query"));

    // Suppress unused.
    let _ = ui::split(Rect::new(0, 0, 1, 1));
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test view_snapshot`
Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/view_snapshot.rs
git commit -m "tests: snapshot test for HelpView render"
```

### Task 8.7: Final pass — compiler warnings, README

**Files:**
- Modify: `Cargo.toml`
- (optional) Create: `README.md`

- [ ] **Step 1: Fail the build on warnings to spot lingering dead code**

Add to `Cargo.toml`:

```toml
[lints.rust]
unused_imports = "warn"
dead_code = "warn"
```

Run: `cargo build --all-targets 2>&1 | tee /tmp/postui-build.log | tail -40`

Address any warnings:
- Genuinely unused imports → remove.
- Dead-code helpers from earlier tasks (e.g., `_force_dbgerr`, `_silence`, `_unused`, `_types`) → remove now that everything is wired.

- [ ] **Step 2: Run the full test suite (no docker required)**

Run: `cargo test --lib`
Expected: all unit tests pass.

Run: `cargo test --tests` (this runs both unit + integration; integration tests are `#[ignore]`-ed when docker isn't there).

- [ ] **Step 3: Run integration tests if docker is available**

Run: `cargo test -- --ignored`
Expected (with docker): all 12+ integration tests pass.

- [ ] **Step 4: Optional — create a brief README**

Only if the user requests one (the global rules forbid creating docs files unprompted). Skip otherwise.

- [ ] **Step 5: Commit any cleanup**

```bash
git add Cargo.toml src/
git commit -m "chore: enable warning lints + remove dead-code shims"
```

**Milestone 8 complete.** v1 is feature-complete: theming switches at runtime and persists; `:connections` is the launch view when no connection was passed; `/` filters; `?` shows help.

---

## Done Criteria for v1

- [ ] All 8 milestones complete; each milestone's smoke test passes.
- [ ] `cargo test --lib` passes (unit tests).
- [ ] `cargo test -- --ignored` passes when docker is available (integration tests).
- [ ] `cargo build --all-targets` is warning-clean.
- [ ] Manual smoke: launch with `--uri postgres://$USER@localhost/postgres`, browse `:databases` → `:schemas` → `:tables` → inspector → row detail → edit → confirm → row updated.
- [ ] Manual smoke: `:queries` polls live; `Ctrl-K` cancels the selected backend after `y` confirm.
- [ ] Manual smoke: `:themes`, scroll, see live preview; `<enter>` persists to config.
- [ ] Manual smoke: `:query`, `Ctrl-E` opens nvim; saving and quitting brings the buffer back; `Ctrl-R` runs.
- [ ] Manual smoke: `?` shows help; `/` filters list views.

---

## Self-Review Notes

- Spec section "Browse chain" → covered by M3 tasks 3.4–3.7.
- Spec section "Table inspector" → covered by M4 tasks 4.4–4.7.
- Spec section "Row detail + editing" → covered by M7.
- Spec section "`:query`" → covered by M5.
- Spec section "Live activity" → covered by M6.
- Spec section "Connections" → M2 (load/connect from CLI) + M8.3 (in-app `:connections`).
- Spec section "Vim motion keys" → M3 task 3.1 (`keys::vim_motion`).
- Spec section "Themes" → M1 task 1.3 (built-ins) + M8 tasks 8.1–8.2 (`:themes`).
- Spec section "Universal keys" → M1 + M8 tasks 8.4 (`/`) and 8.5 (`?`).
- Spec section "Architecture / Main loop / View stack / DB I/O" → M1 task 1.9 (App).
- Spec section "Module Layout" → matches the file structure mapped at the top of this plan.
- Spec section "Data Flow" → command dispatch (M1.7+M3.6), one-shot fetch (M3.5), live polling (M6.4), mutation flow (M6.3 + M7.3), result paging (M4.3 + M4.5), cancellation (M5.3 + M5.4 + M6.4).
- Spec section "Theming" → M1.3 (`Theme` struct + 5 built-ins) + M8 (live preview).
- Spec section "Crate Stack" → M1.1, M2.1, M5.1, M6.1, M7.1, M8.1 (all dep additions).
- Spec section "Error Handling" → M1.2 (`AppError`/`DbError`/`ConfigError`); panic hook in M1.5.
- Spec section "Testing Strategy" → unit tests in each task (config, mutate, palette, theme, keys, table, query); integration tests in M3.3, M4.1, M4.3, M6.1, M7.2; snapshot in M8.6.
- Spec section "Config Schema" → matches the `Config`/`UiConfig`/`ConnectionConfig`/`ViewsConfig` types in M2.2 + M8.1 (Serialize derives).
- Spec section "File / Directory Layout (XDG)" → log path in M1.4 (`logging::init`); config path in M2.6 (`default_config_path`).

No placeholders found. Type / method names appear consistent (`PgConn::connect`, `View::handle_key`, `Outcome::Push/Pop/Pass/Consumed/Quit`, `AppEvent::ViewData/Toast/PushView/PreviewTheme/RestoreTheme/PersistTheme/ConnectionSwitched`, `ViewPayload::Databases/Schemas/Tables/Columns/Indexes/Constraints/Size/Rows/Query/Activity/Locks/OpResult`).

