pub mod confirm;
pub mod detail;
pub mod editor;
pub mod footer;
pub mod header;
pub mod palette;
pub mod table;
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

pub fn render_main_placeholder(f: &mut Frame, area: Rect) {
    use ratatui::widgets::{Block, Borders};
    f.render_widget(Block::default().borders(Borders::NONE), area);
}
