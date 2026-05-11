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
        let modal = crate::ui::centered_rect(70, 50, area);
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
