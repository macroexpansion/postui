//! :help — modal listing keybindings and palette commands.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    ui::{self, theme::Theme},
    views::{Ctx, Modal, ModalOutcome},
};

const TEXT: &str = "
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

  Press Esc key to dismiss.
";

// --- Modal (new) ---

pub struct HelpModal {
    scroll: u16,
    viewport: u16,
}

impl HelpModal {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            viewport: 0,
        }
    }

    fn max_scroll(&self) -> u16 {
        let lines = TEXT.lines().count() as u16;
        lines.saturating_sub(self.viewport)
    }

    fn page(&self) -> u16 {
        self.viewport.saturating_sub(1).max(1)
    }
}

impl Default for HelpModal {
    fn default() -> Self {
        Self::new()
    }
}

impl Modal for HelpModal {
    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let rect = ui::centered_rect(70, 80, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" quick help ")
            .border_style(Style::default().fg(theme.border));
        let inner = block.inner(rect);
        self.viewport = inner.height;
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
        }
        let p = Paragraph::new(TEXT)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        f.render_widget(p, rect);
    }

    fn handle_key(&mut self, key: KeyEvent, _ctx: &mut Ctx) -> ModalOutcome {
        match key.code {
            KeyCode::Esc => ModalOutcome::Close,
            KeyCode::Char('j') => {
                self.scroll = (self.scroll + self.page()).min(self.max_scroll());
                ModalOutcome::Consumed
            }
            KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(self.page());
                ModalOutcome::Consumed
            }
            _ => ModalOutcome::Consumed,
        }
    }

    fn hints(&self) -> &str {
        "j/k page down/up • Esc close"
    }
}
