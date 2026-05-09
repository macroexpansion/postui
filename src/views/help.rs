//! :help — modal listing keybindings and palette commands.

use crossterm::event::KeyEvent;
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
