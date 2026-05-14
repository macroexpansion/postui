//! Row detail: 2-column key/value form, editable in-place.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table as RTable, TableState},
};

use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    View,
    Edit,
}

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
        Self {
            fields,
            state,
            mode: Mode::View,
        }
    }

    pub fn move_up(&mut self) {
        if let Some(i) = self.state.selected() {
            self.state.select(Some(i.saturating_sub(1)));
        }
    }

    pub fn move_down(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        let next = (i + 1).min(self.fields.len() - 1);
        self.state.select(Some(next));
    }

    pub fn enter_edit(&mut self) {
        self.mode = Mode::Edit;
    }

    pub fn leave_edit(&mut self) {
        self.mode = Mode::View;
    }

    pub fn append_char(&mut self, c: char) {
        if self.mode == Mode::Edit
            && let Some(i) = self.state.selected()
            && let Some(f) = self.fields.get_mut(i)
            && !f.is_pk
        {
            f.edited.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.mode == Mode::Edit
            && let Some(i) = self.state.selected()
            && let Some(f) = self.fields.get_mut(i)
            && !f.is_pk
        {
            f.edited.pop();
        }
    }

    /// Returns dirty fields (edited != original AND not PK).
    pub fn dirty(&self) -> Vec<&DetailField> {
        self.fields
            .iter()
            .filter(|f| !f.is_pk && f.edited != f.original)
            .collect()
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let header = Row::new(vec![Cell::from("column"), Cell::from("value")]).style(
            Style::default()
                .fg(theme.table_header)
                .add_modifier(Modifier::BOLD),
        );

        let mode = self.mode;
        let body: Vec<Row> = self
            .fields
            .iter()
            .map(|fld| {
                let val = if mode == Mode::Edit && !fld.is_pk {
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
            })
            .collect();

        let widths = vec![Constraint::Percentage(30), Constraint::Percentage(70)];
        let title = match self.mode {
            Mode::View => " row detail [VIEW] ",
            Mode::Edit => " row detail [EDIT — up/down moves, Enter saves, Esc cancels] ",
        };
        let table = RTable::new(body, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(theme.border)),
            )
            .row_highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg),
            );

        f.render_stateful_widget(table, area, &mut self.state);
    }
}
