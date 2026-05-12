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
        let rows = config
            .connections
            .iter()
            .map(|c| {
                let active_mark = if active == Some(c.name.as_str()) {
                    "*"
                } else {
                    ""
                };
                vec![
                    active_mark.into(),
                    c.name.clone(),
                    c.host
                        .clone()
                        .unwrap_or_else(|| c.url.as_deref().map(short_host).unwrap_or_default()),
                    c.user.clone().unwrap_or_default(),
                    c.database.clone().unwrap_or_default(),
                ]
            })
            .collect();
        table.set_rows(rows);
        Self {
            id: ViewId::next(),
            table,
            names: config.connections.iter().map(|c| c.name.clone()).collect(),
        }
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.table
            .selected_index()
            .and_then(|i| self.names.get(i))
            .map(String::as_str)
    }
}

fn short_host(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_else(|| "(uri)".into())
}

impl View for ConnectionsView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn title(&self) -> &str {
        "connections"
    }

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

    fn set_filter(&mut self, filter: &str) {
        self.table.set_filter(filter);
    }
    fn supports_filter(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
