# postui — Design Spec

**Date:** 2026-05-08
**Status:** Draft, pending implementation

## Summary

`postui` is a terminal UI for PostgreSQL modeled after [k9s](https://k9scli.io/): a single-pane, command-driven interface for browsing database state and watching live activity. Built in Rust on `ratatui` + `tokio` + `tokio-postgres`.

## Goals

- Keyboard-first, k9s-style navigation: one main pane at a time; switch components with `:command`; drill in with `<enter>`, back with `<esc>`.
- Live operational views (`:queries`, `:locks`, `:sessions`) that auto-refresh while visible.
- Browse the catalog (databases → schemas → tables) and inspect tables in depth (columns, indexes, constraints, sizes).
- Run ad-hoc SQL with a built-in editor that escapes to `$EDITOR` for serious work.
- Edit rows in place with confirmation, k9s-style.
- Switchable themes at runtime.

## Non-goals (v1)

- Non-Postgres databases (MySQL, SQLite, etc.). Driver trait will be extracted in v2 once we know what shape it needs to be from real Postgres-only use.
- Persisted query history / favorites.
- Read-only mode flag, environment markers (e.g., red bar for prod), explicit transaction ceremony, dry-run mode. v1 ships a single confirm-on-execute and trusts the operator.
- `:explain`, `:views`, `:functions`, schema diff, CSV/JSON export.
- Vim-mode in the `:query` editor (Normal/Insert split). The escape hatch to `$EDITOR` covers this.
- User-defined theme files. v1 ships built-in themes only.
- SQL syntax highlighting in the built-in editor.
- Server-side cursors for paging large result sets. v1 uses simple `LIMIT`/`OFFSET`.

## MVP Feature List

1. **Browse chain:** `:connections`, `:databases`, `:schemas`, `:tables`, then drill into a table.
2. **Table inspector:** tabs `rows | columns | indexes | constraints | size`.
3. **Row detail + editing:** `<enter>` opens row detail; `i` edits, `a` inserts a new row, `d` deletes. Mutations show generated SQL and ask `y/N`.
4. **`:query`:** built-in multi-line editor + result pane. `Ctrl-R` (or `F5`) runs, `Ctrl-E` opens buffer in `$EDITOR`, `Ctrl-C` cancels in-flight query.
5. **Live activity:** `:queries`, `:locks`, `:sessions` (pg_stat_activity / pg_locks). Auto-refresh while visible (default 2s, per-view configurable). `Ctrl-K` cancels a backend (`pg_cancel_backend`); `:terminate <pid>` from the palette terminates (`pg_terminate_backend`).
6. **Connections:** TOML config with discrete fields *or* full `postgres://...` URI form. `${ENV_VAR}` interpolation for secrets, prompt fallback if a password isn't resolvable. `:connect` opens an in-app form that also accepts a full URI.
7. **Vim motion keys** in list/table views: `j`/`k` (up/down), `h`/`l` (tab switch / column scroll), `w`/`b` (page-jump down/up ~10 rows), `e` (jump to last row).
8. **Themes:** 5 built-in themes (`default`, `dracula`, `gruvbox-dark`, `nord`, `solarized-dark`). `:themes` view with **live preview as you cursor through**; `<enter>` persists, `<esc>` reverts. Or one-shot `:theme <name>`.
9. **Universal keys:** `:` palette, `<esc>` pop view, `?` help, `/` filter visible rows, `Ctrl-Q` or `:q` to quit, `Ctrl-C` cancels in-flight task.

## Architecture

### Runtime model

Current-thread `tokio` runtime (`Builder::new_current_thread`). The whole app lives in one main loop; DB operations are spawned as tasks on the same runtime and report back via channels. **Only the main loop mutates app state** — no `Arc<Mutex<App>>`. We don't need a worker pool for an interactive TUI's load.

### Main loop

```
loop:
  select!:
    key       = crossterm event stream     -> dispatch_key(key)
    msg       = app_event_rx.recv()        -> handle_event(msg)
    _         = tick_interval.tick()       -> on_tick()
  terminal.draw(|f| app.render(f))
```

Three input sources, one writer.

### View stack

```rust
struct App {
    config: Config,
    conn: Option<PgConn>,
    views: Vec<Box<dyn View>>,         // .last_mut() is active
    palette: Palette,
    status: StatusBar,
    theme: &'static Theme,
    app_event_tx: mpsc::Sender<AppEvent>,
}

trait View {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme);
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome;
    fn on_tick(&mut self, ctx: &mut Ctx);   // for live-refresh views
    fn on_enter(&mut self, ctx: &mut Ctx);  // start polling, kick off initial fetch
    fn on_leave(&mut self, ctx: &mut Ctx);  // stop polling
}

enum Outcome { Consumed, Push(Box<dyn View>), Pop, Quit, Pass }
```

- `:command` → push a new view. `<esc>` → pop. Quit when stack empties.
- The active view receives keys; the palette intercepts when `:` is pressed.
- `on_enter`/`on_leave` give live views (`:queries`, `:locks`, `:sessions`) a hook to start/stop background pollers, so we don't query Postgres for views nobody's looking at.

### DB I/O

- `PgConn` wraps `tokio_postgres::Client` (one async connection per active connection profile).
- All DB calls return through `mpsc::Sender<AppEvent>`; the main loop applies results to view state.
- Each spawned DB task carries a `CancellationToken`; `Ctrl-C` in a view cancels its in-flight task without quitting the app.
- Each view has a unique `view_id` assigned at construction. Stale results (user navigated away) are silently dropped.
- Live pollers are `tokio::spawn`-ed JoinHandles; `on_leave` cancels their tokens.

## Module Layout

```
src/
  main.rs              entry: load config, build runtime, run App
  app.rs               App struct, event loop, view stack, dispatch
  config.rs            TOML schema, env-var interpolation, URI parsing
  error.rs             thiserror-based error types
  keys.rs              KeyMap + bindings (vim motions live here)

  db/
    mod.rs             PgConn (wraps tokio_postgres::Client), connect()
    catalog.rs         pg_catalog queries: schemas, tables, columns, indexes, constraints, sizes
    activity.rs        pg_stat_activity, pg_locks, sessions
    rows.rs            row fetch with LIMIT/OFFSET paging; cancel
    mutate.rs          UPDATE/INSERT/DELETE SQL builders for row editor
    types.rs           postgres::Row -> Vec<DisplayValue> conversion

  ui/
    mod.rs             top-level layout (header / main / footer)
    header.rs          connection name + breadcrumb
    footer.rs          context-sensitive keybind hints + status
    palette.rs         ":command" line: input, parsing, completion
    table.rs           generic ratatui-based data table widget
    editor.rs          tui-textarea wrapper + Ctrl-E -> $EDITOR shell-out
    confirm.rs         modal "execute? y/N" with SQL preview
    detail.rs          row-detail page (key/value form)
    theme.rs           struct Theme + built-in constants + by_name()

  views/
    mod.rs             View trait, Outcome, Ctx
    connections.rs     :connections — list connection profiles
    databases.rs       :databases — \l equivalent
    schemas.rs         :schemas — pg_namespace
    tables.rs          :tables — pg_class WHERE relkind in ('r','p')
    table_inspector.rs row-detail entry; tabs: rows | columns | indexes | constraints | size
    rows.rs            paged row view; i/a/d to mutate
    query.rs           :query — editor + result pane
    activity.rs        :queries / :locks / :sessions
    themes.rs          :themes — list + live preview + persist
```

**Conventions:**
- Generic widgets in `ui/` (table, editor, confirm, detail) are agnostic of what they display — every list view uses `ui::table`.
- Catalog queries return concrete typed structs (`TableInfo`, `ColumnInfo`, `ActivityRow`); views render them. No leaking `tokio_postgres::Row` past `db/`.
- `views/` modules are thin: state struct + `View` impl + the catalog call(s) they fire on `on_enter`/`on_tick`. Heavy lifting lives in `db/` and `ui/`.
- One file per `:command`. When a view exceeds ~300 lines it's a signal something belongs in `ui/` or `db/`.

## Data Flow

### 1. Command dispatch

```
key ':' pressed
  -> palette enters input mode, captures keys until Enter/Esc
Enter
  -> palette parses ":tables" -> Cmd::Open("tables")
  -> App::dispatch_cmd:
       active.on_leave(ctx)          // stops old view's pollers
       construct new view (e.g., TablesView::new(current_schema))
       views.push(new)
       new.on_enter(ctx)             // kicks off initial fetch
```

### 2. One-shot fetch (lists, query results)

```
TablesView::on_enter:
  ctx.spawn(async move {
    let rows = db::catalog::list_tables(&conn, &schema).await;
    tx.send(AppEvent::TablesLoaded { view_id, result: rows }).await;
  });
  state = Loading;

main loop receives AppEvent::TablesLoaded { view_id, result }:
  if view_id matches active view -> view.apply(result)
  else                            -> drop (user navigated away)
```

Failures: `result` is `Result<Vec<TableInfo>, DbError>`. The view renders a one-line error in its body; `?` shows the full message in a modal.

### 3. Live polling (activity views)

```
ActivityView::on_enter:
  let token = CancellationToken::new();
  let handle = tokio::spawn(async move {
    let mut tick = interval(Duration::from_secs(2));
    loop {
      select! {
        _ = token.cancelled() => break,
        _ = tick.tick() => {
          let rows = db::activity::pg_stat_activity(&conn).await;
          tx.send(AppEvent::ActivityTick { view_id, rows }).await;
        }
      }
    }
  });
  self.poller = Some((handle, token));

ActivityView::on_leave:
  if let Some((_, token)) = self.poller.take() { token.cancel(); }
```

Polling stops the moment you switch away. Cadence is per-view config (default 2s).

### 4. Mutation flow

```
user submits change
  -> view builds SQL via db::mutate (parameterized)
  -> Confirm modal pushed: shows the SQL + estimated row count
  -> on 'y':
      ctx.spawn(async move {
        let res = conn.execute(sql, params).await;
        tx.send(AppEvent::MutationResult { view_id, result }).await;
      });
  -> result toast in footer ("UPDATE 1 — 14ms" or full error)
  -> view re-fetches itself (on_enter again) to reflect the change
```

### 5. Result paging

- Page size: 100 rows.
- Server-side `LIMIT 100 OFFSET <page * 100>` per fetch.
- Footer: `rows 1–100 (of N)`.
- `PageDown` / `w` advances; `PageUp` / `b` goes back.
- No server-side cursors in v1. Upgrade path if large tables become painful.

### 6. Cancellation

Every spawned DB task carries a `CancellationToken`. `Ctrl-C` cancels the in-flight task for the current view. Quit is `Ctrl-Q` or `:q`.

## Per-View Sketches

Every list view uses `ui::table`. Only the columns and key bindings vary.

- **`:connections`** — profiles from config + active marked `*`. `<enter>` connects/switches. `i` edits, `a` adds, `d` deletes (writes back to config; secrets shown as `${VAR}`, never resolved values).
- **`:databases`** — `SELECT datname FROM pg_database WHERE NOT datistemplate`. `<enter>` switches active DB.
- **`:schemas`** — `pg_namespace` filtered (no `pg_*`, no `information_schema`). `<enter>` sets current schema and auto-pushes `:tables`.
- **`:tables`** — `pg_class` `relkind in ('r','p')`. Columns: name, rows (estimate from `reltuples`), size (`pg_total_relation_size`). `<enter>` pushes the **table inspector**.
- **Table inspector** — top tab bar `[ rows | columns | indexes | constraints | size ]`. `h`/`l` switch tabs, `j`/`k` navigates within the active tab.
  - **rows:** paged 100/page; `<enter>` opens row-detail; `i`/`a`/`d` for edit/insert/delete.
  - **columns:** name / type / nullable / default / comment.
  - **indexes:** name / definition / size / scans (from `pg_stat_user_indexes`).
  - **constraints:** name / type / definition (PK / FK / check / unique).
  - **size:** total / heap / indexes / toast / row count, plus bloat estimate if cheap.
- **Row detail** — full row as key/value form. `i` enters edit; field-by-field tab navigation. Submit → mutation flow with SQL preview.
- **`:query`** — split layout: editor on top, results below. `Ctrl-R` (or `F5`) runs. `Ctrl-E` opens buffer in `$EDITOR`. Multiple result sets shown as tabs at the top of the result pane. `Ctrl-C` cancels in-flight query.
- **`:queries`** — `pg_stat_activity` filtered to non-idle non-self rows. Columns: pid, user, db, state, duration, wait_event, query (truncated). `<enter>` opens full query in a modal. `Ctrl-K` runs `pg_cancel_backend(pid)` for the selected row. Termination goes through the palette: `:terminate <pid>` (deliberately more friction for the destructive op). Both go through the confirm modal.
- **`:locks`** — `pg_locks` joined to `pg_stat_activity`. Columns: pid, mode, granted, relation, query. Same `Ctrl-K` cancel and `:terminate <pid>` semantics as `:queries`.
- **`:sessions`** — `pg_stat_activity` unfiltered (idle included). Same shape as `:queries`, no per-row cancel by default.
- **`:themes`** — list of theme names. Cursor movement live-previews the theme on the whole UI. `<enter>` persists to config; `<esc>` reverts.

## Theming

```rust
pub struct Theme {
    pub bg:           Color,
    pub fg:           Color,
    pub border:       Color,
    pub header:       Color,
    pub footer:       Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub accent:       Color,   // active tab, palette ':' cursor
    pub muted:        Color,
    pub error:        Color,
    pub warn:         Color,
    pub success:      Color,
    pub table_header: Color,
    pub row_stripe:   Color,
}
```

- All renders look up colors from `Theme` — never hardcoded.
- 5 built-in themes shipped as `&'static Theme` constants in `ui/theme.rs`: `default`, `dracula`, `gruvbox-dark`, `nord`, `solarized-dark`.
- `App` holds `theme: &'static Theme`. Switching = swap the pointer; next render uses new colors. No restart needed.
- The built-in `:query` editor is unstyled in v1 (no syntax highlighting). `Ctrl-E → nvim` handles real editing.

## Crate Stack

- `tokio` (`rt`, `macros`, `sync`, `time`, `signal`) — current-thread runtime; we don't need a worker pool
- `ratatui` — TUI
- `crossterm` — terminal backend
- `tokio-postgres` + `postgres-types` — DB driver. Chose over `sqlx` because queries are dynamic / user-supplied; sqlx's compile-time check macros buy us nothing here, and `tokio-postgres` exposes pg-specific bits (cancel tokens, NOTIFY) we'll want later.
- `tui-textarea` — multi-line editor for `:query`
- `serde` + `toml` — config
- `url` — parse `postgres://...` URIs
- `thiserror` — error types
- `tracing` + `tracing-appender` — logging to file (stdout owned by TUI). Default `~/.local/state/postui/postui.log`.
- `tokio-util` (`CancellationToken`)
- `directories` — XDG paths
- `clap` (derive) — CLI: `postui [--config PATH] [--connection NAME | postgres://...URI]`. `--connection` and a positional URI are mutually exclusive; if neither is given, the app opens at `:connections`.

## Error Handling

- `enum DbError` (`thiserror`) for `db/`: `Connect`, `Query { sql, source }`, `Type`, `Cancelled`. Carries failing SQL when relevant so the error modal can show it.
- `enum ConfigError` for `config.rs`: `Read`, `Parse`, `MissingEnv { var }`, `BadUri`.
- Top-level `Result<T> = std::result::Result<T, AppError>` with `From` impls.
- Errors are *never* fatal except at startup. Mid-session DB errors render in the affected view's body or a transient footer toast; the user can retry or `:command` away.
- Panics: `std::panic::set_hook` to restore the terminal cleanly, write the panic to the log file, then re-panic.

## Testing Strategy

- **Unit (no DB):**
  - `config.rs`: parsing, env interpolation, URI round-trip, error cases.
  - `db/mutate.rs`: SQL generation for UPDATE/INSERT/DELETE given a row + edits.
  - `ui/palette.rs`: command parsing.
  - `keys.rs`: keymap dispatch table.
  - `ui/theme.rs`: lookup by name.
- **Integration (real Postgres via `testcontainers-rs`):**
  - `db/catalog.rs`: list_schemas / list_tables / list_columns / list_indexes / list_constraints against a fixture DB.
  - `db/activity.rs`: `pg_stat_activity` and `pg_locks` shape.
  - `db/rows.rs`: paged fetch + type conversion across common pg types (int, text, timestamptz, jsonb, uuid, numeric, arrays, bytea).
  - `db/mutate.rs` end-to-end: build SQL, execute, re-fetch, verify.
- **View tests:** `View::handle_key` is pure-ish (state + key in, state + Outcome out). Snapshot-test renders with `ratatui::backend::TestBackend` for the table widget and a few representative views.
- No e2e TUI driver in v1. Integration + view tests cover the regressions that matter.

## Config Schema

```toml
# ~/.config/postui/config.toml

[ui]
theme     = "dracula"   # one of: default | dracula | gruvbox-dark | nord | solarized-dark
tick_ms   = 2000        # default live-view refresh cadence
page_size = 100         # rows per page in rows view / :query results

[[connection]]
name     = "local"
host     = "localhost"
port     = 5432
user     = "andrew"
database = "app_dev"
# password omitted -> prompted on first :connect

[[connection]]
name     = "prod"
host     = "db.prod.internal"
port     = 5432
user     = "andrew"
database = "app"
password = "${PG_PROD_PASSWORD}"
sslmode  = "require"

# alternate URI form, equivalent:
[[connection]]
name = "stage"
url  = "postgres://andrew:${PG_STAGE_PASSWORD}@db.stage:5432/app?sslmode=require"

# per-view overrides (optional)
[views.queries]
tick_ms = 1000
```

## File / Directory Layout (XDG)

- Config: `~/.config/postui/config.toml`
- Log:    `~/.local/state/postui/postui.log`
- (No cache or persisted query history in v1.)

## Open Questions / Future Work

- **Driver abstraction (v2):** extract a `Driver` trait once we have lived with the Postgres-only shape. MySQL is the next likely target.
- **User-defined themes (v2):** load `~/.config/postui/themes/*.toml` alongside the built-ins.
- **Server-side cursors:** if `LIMIT`/`OFFSET` paging becomes painful on large tables.
- **Vim-mode in `:query` editor:** Normal/Insert split with full motion + edit chords. Currently relies on `Ctrl-E → nvim`.
- **`:explain`:** capture a query and run `EXPLAIN (ANALYZE, BUFFERS)`, render the plan tree.
- **CSV/JSON export** of result sets.
- **Read-only flag and env markers** (red bar for prod) — deliberately out of v1.
- **Query history / favorites.**
