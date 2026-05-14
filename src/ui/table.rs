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

    /// Set a substring filter; rows not containing the substring (case
    /// insensitive) in any cell are hidden. Empty string clears the filter.
    /// Always resets the cursor to the first visible row.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.recompute_visible_reset();
    }

    /// Replace rows. Recomputes the visible set against the current filter and
    /// preserves the cursor on the underlying row when it remains visible.
    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        // Resolve to the underlying row index using the OLD `visible` before
        // we mutate it.
        let prev_real = self.selected_index();
        self.rows = rows;
        self.recompute_visible_preserving(prev_real);
    }

    fn recompute_visible_inner(&mut self) {
        if self.filter.is_empty() {
            self.visible = (0..self.rows.len()).collect();
        } else {
            self.visible = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.iter().any(|c| c.to_lowercase().contains(&self.filter)))
                .map(|(i, _)| i)
                .collect();
        }
    }

    fn recompute_visible_reset(&mut self) {
        self.recompute_visible_inner();
        if self.visible.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }

    fn recompute_visible_preserving(&mut self, prev_real: Option<usize>) {
        self.recompute_visible_inner();
        if self.visible.is_empty() {
            self.state.select(None);
        } else {
            let new_v = prev_real
                .and_then(|r| self.visible.iter().position(|&i| i == r))
                .unwrap_or(0);
            self.state.select(Some(new_v));
        }
    }

    /// Return the underlying row index (post-filter translation) that the
    /// cursor points at, or `None` when nothing is visible.
    pub fn selected_index(&self) -> Option<usize> {
        let v = self.state.selected()?;
        self.visible.get(v).copied()
    }

    pub fn selected_row(&self) -> Option<&[String]> {
        self.selected_index()
            .and_then(|i| self.rows.get(i))
            .map(|v| v.as_slice())
    }

    pub fn move_motion(&mut self, m: Motion) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
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
        let header_row = Row::new(
            self.headers
                .iter()
                .cloned()
                .map(Cell::from)
                .collect::<Vec<_>>(),
        )
        .style(
            Style::default()
                .fg(theme.table_header)
                .add_modifier(Modifier::BOLD),
        );

        let body: Vec<Row> = self
            .visible
            .iter()
            .map(|&i| {
                Row::new(
                    self.rows[i]
                        .iter()
                        .cloned()
                        .map(Cell::from)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        let widths: Vec<Constraint> = self
            .headers
            .iter()
            .map(|_| Constraint::Percentage(100 / self.headers.len().max(1) as u16))
            .collect();

        let table = RTable::new(body, widths)
            .header(header_row)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            )
            .row_highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg)
                    .add_modifier(Modifier::BOLD),
            );

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

    #[test]
    fn filter_hides_non_matching_rows() {
        let mut t = populated();
        t.set_filter("y");
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

    #[test]
    fn set_rows_preserves_cursor_when_row_still_present() {
        let mut t = populated();
        t.move_motion(Motion::Down); // cursor at row 1
        assert_eq!(t.selected_index(), Some(1));
        // Refresh with the SAME rows → cursor stays at 1.
        t.set_rows(vec![
            vec!["1".into(), "x".into()],
            vec!["2".into(), "y".into()],
            vec!["3".into(), "z".into()],
        ]);
        assert_eq!(t.selected_index(), Some(1));
    }

    #[test]
    fn set_rows_preserves_cursor_through_active_filter() {
        let mut t = populated();
        t.set_filter("y"); // visible = [1], selected_index = Some(1)
        assert_eq!(t.selected_index(), Some(1));
        // Refresh with the same rows. Filter still active; selection should remain on row index 1.
        t.set_rows(vec![
            vec!["1".into(), "x".into()],
            vec!["2".into(), "y".into()],
            vec!["3".into(), "z".into()],
        ]);
        assert_eq!(t.selected_index(), Some(1));
        assert_eq!(t.visible, vec![1]);
    }

    #[test]
    fn set_rows_drops_cursor_to_first_visible_when_real_row_disappears() {
        let mut t = populated();
        t.set_filter("y"); // visible = [1], selected_index = Some(1)
        // Replace rows: now NO row contains "y", but two contain "x".
        t.set_rows(vec![
            vec!["1".into(), "x".into()],
            vec!["a".into(), "x".into()],
        ]);
        // Filter still applies (lowercased "y") — no rows match. Selection should be None.
        assert_eq!(t.selected_index(), None);
        assert!(t.visible.is_empty());
    }

    #[test]
    fn set_rows_with_active_filter_falls_back_to_first_visible_when_real_row_gone() {
        let mut t = populated();
        t.set_filter("y"); // selected real-row = 1
        // New rows: row 1 no longer has "y", but a different row does.
        t.set_rows(vec![
            vec!["a".into(), "x".into()],
            vec!["b".into(), "z".into()],
            vec!["c".into(), "y".into()],
        ]);
        // Filter still "y": only new row 2 matches. Real-row 1 is gone, so cursor falls back to first visible.
        assert_eq!(t.visible, vec![2]);
        assert_eq!(t.selected_index(), Some(2));
    }
}
