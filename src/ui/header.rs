use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use super::theme::Theme;

pub fn render(f: &mut Frame, area: Rect, theme: &Theme, title: &str, breadcrumb: &str) {
    let text = if breadcrumb.is_empty() {
        format!("{title:<24}")
    } else {
        format!("{title:<24} > {breadcrumb}")
    };
    let p = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        )
        .style(Style::default().fg(theme.header));
    f.render_widget(p, area);
}
