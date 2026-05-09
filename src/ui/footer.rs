use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
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
    let border = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    if palette.open {
        let typed = Span::styled(
            format!(":{}", palette.buffer),
            Style::default().fg(theme.accent),
        );
        let ghost = Span::styled(
            palette.suggestion.as_deref().unwrap_or(""),
            Style::default().fg(theme.muted),
        );
        let p = Paragraph::new(Line::from(vec![typed, ghost])).block(border);
        f.render_widget(p, area);
        return;
    }

    let (line, style) = if let Some(t) = toast {
        (t.to_string(), Style::default().fg(theme.warn))
    } else {
        (hints.to_string(), Style::default().fg(theme.footer))
    };

    let p = Paragraph::new(line).block(border).style(style);
    f.render_widget(p, area);
}
