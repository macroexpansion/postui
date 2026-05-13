//! ":command" palette state and parser.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState},
};

use super::theme::Theme;

#[derive(Debug, Clone, Copy)]
enum ArgKind {
    None,
    ThemeNames,
    ConnectionNames,
    Freeform,
}

struct CommandDef {
    name: &'static str,
    aliases: &'static [&'static str],
    arg_kind: ArgKind,
}

static COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "databases",
        aliases: &["db"],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "schemas",
        aliases: &["sc"],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "tables",
        aliases: &["tb"],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "query",
        aliases: &["sql"],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "queries",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "locks",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "sessions",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "connections",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "themes",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "help",
        aliases: &[],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "quit",
        aliases: &["q"],
        arg_kind: ArgKind::None,
    },
    CommandDef {
        name: "theme",
        aliases: &[],
        arg_kind: ArgKind::ThemeNames,
    },
    CommandDef {
        name: "connect",
        aliases: &[],
        arg_kind: ArgKind::ConnectionNames,
    },
    CommandDef {
        name: "terminate",
        aliases: &[],
        arg_kind: ArgKind::Freeform,
    },
];

static THEME_NAMES: &[&str] = &[
    "default",
    "dracula",
    "gruvbox-dark",
    "nord",
    "solarized-dark",
];

pub fn suggest(buffer: &str) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }
    let (head, tail) = match buffer.split_once(' ') {
        Some((h, t)) => (h, Some(t)),
        None => (buffer, None),
    };

    if let Some(tail) = tail {
        let cmd = COMMANDS
            .iter()
            .find(|c| c.name == head || c.aliases.contains(&head));
        let cmd = cmd?;
        match cmd.arg_kind {
            ArgKind::ThemeNames => {
                let full = THEME_NAMES
                    .iter()
                    .find(|&&n| n.starts_with(tail) && n != tail)?;
                Some(full[tail.len()..].to_string())
            }
            ArgKind::ConnectionNames | ArgKind::Freeform | ArgKind::None => None,
        }
    } else {
        COMMANDS
            .iter()
            .find(|c| c.name.starts_with(head))
            .map(|c| c.name[head.len()..].to_string())
    }
}

#[derive(Debug)]
pub struct Palette {
    pub open: bool,
    pub buffer: String,
    pub suggestion: Option<String>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            open: false,
            buffer: String::new(),
            suggestion: None,
            filtered: Vec::new(),
            selected: 0,
        }
    }
}

impl Palette {
    pub fn open(&mut self) {
        self.open = true;
        self.buffer.clear();
        self.suggestion = None;
        self.rebuild_filtered();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.buffer.clear();
        self.suggestion = None;
        self.filtered.clear();
        self.selected = 0;
    }

    pub fn push(&mut self, c: char) {
        self.buffer.push(c);
        self.suggestion = suggest(&self.buffer);
        self.rebuild_filtered();
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
        self.suggestion = suggest(&self.buffer);
        self.rebuild_filtered();
    }

    pub fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len() - 1;
        } else {
            self.selected -= 1;
        }
        self.sync_buffer_to_selection();
    }

    pub fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected >= self.filtered.len() - 1 {
            self.selected = 0;
        } else {
            self.selected += 1;
        }
        self.sync_buffer_to_selection();
    }

    pub fn select_item(&mut self) {
        if let Some(&idx) = self.filtered.get(self.selected) {
            self.buffer = COMMANDS[idx].name.to_string();
            self.suggestion = suggest(&self.buffer);
            self.rebuild_filtered();
        }
    }

    fn sync_buffer_to_selection(&mut self) {
        if let Some(&idx) = self.filtered.get(self.selected) {
            self.buffer = COMMANDS[idx].name.to_string();
            self.suggestion = suggest(&self.buffer);
        }
    }

    fn rebuild_filtered(&mut self) {
        let head = match self.buffer.split_once(' ') {
            Some((h, _)) => h,
            None => &self.buffer,
        };
        if head.is_empty() {
            self.filtered = (0..COMMANDS.len()).collect();
        } else {
            self.filtered = COMMANDS
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name.starts_with(head))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
    }
}

pub fn render_dropdown(f: &mut Frame, area: Rect, theme: &Theme, palette: &Palette) {
    if !palette.open || palette.buffer.contains(' ') || palette.filtered.is_empty() {
        return;
    }

    let max_item_width: usize = palette
        .filtered
        .iter()
        .map(|&i| {
            let cmd = &COMMANDS[i];
            if cmd.aliases.is_empty() {
                cmd.name.len()
            } else {
                let aliases_str = cmd.aliases.join(", ");
                cmd.name.len() + 3 + aliases_str.len()
            }
        })
        .max()
        .unwrap_or(0);

    let width = (max_item_width as u16 + 8)
        .min(50)
        .min(area.width.saturating_sub(1));
    let visible = (palette.filtered.len() as u16).min(12);
    let height = visible.min(area.height);
    let y = area.bottom().saturating_sub(height);
    let x = area.x;
    let rect = Rect::new(x, y, width, height);

    f.render_widget(Clear, rect);

    let bg = Block::default().style(Style::default().bg(theme.bg));
    let inner = bg.inner(rect);
    f.render_widget(bg, rect);

    let items: Vec<ListItem> = palette
        .filtered
        .iter()
        .map(|&i| {
            let cmd = &COMMANDS[i];
            if cmd.aliases.is_empty() {
                ListItem::new(Span::styled(
                    cmd.name.to_string(),
                    Style::default().fg(theme.fg),
                ))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(cmd.name.to_string(), Style::default().fg(theme.fg)),
                    Span::styled(
                        format!(" ({})", cmd.aliases.join(", ")),
                        Style::default().fg(theme.muted),
                    ),
                ]))
            }
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(palette.selected));

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(theme.selection_bg)
            .fg(theme.selection_fg)
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, inner, &mut state);
}

/// A parsed palette command.
#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Quit,
    Open(String),            // :tables, :databases, ...
    Theme(String),           // :theme dracula
    Terminate(i32),          // :terminate <pid>
    Connect(Option<String>), // :connect [uri-or-name]
    Unknown(String),
}

/// Parse the buffer (without leading ':') into a Cmd.
pub fn parse(input: &str) -> Cmd {
    let s = input.trim();
    if s.is_empty() {
        return Cmd::Unknown(String::new());
    }
    let mut parts = s.split_whitespace();
    let head = parts.next().unwrap();
    let rest: Vec<&str> = parts.collect();

    match head {
        "q" | "quit" => Cmd::Quit,
        "theme" => Cmd::Theme(rest.join(" ")),
        "terminate" => match rest.first().and_then(|s| s.parse::<i32>().ok()) {
            Some(pid) => Cmd::Terminate(pid),
            None => Cmd::Unknown(s.to_string()),
        },
        "connect" => {
            let arg = rest.join(" ");
            Cmd::Connect(if arg.is_empty() { None } else { Some(arg) })
        }
        // Bare verbs: :databases, :schemas, :tables, :query, :queries, :locks,
        // :sessions, :connections, :themes, :help
        other => Cmd::Open(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quit_aliases() {
        assert_eq!(parse("q"), Cmd::Quit);
        assert_eq!(parse("quit"), Cmd::Quit);
        assert_eq!(parse("  q  "), Cmd::Quit);
    }

    #[test]
    fn parses_open_verbs() {
        assert_eq!(parse("tables"), Cmd::Open("tables".into()));
        assert_eq!(parse("databases"), Cmd::Open("databases".into()));
        assert_eq!(parse("queries"), Cmd::Open("queries".into()));
    }

    #[test]
    fn parses_theme_with_arg() {
        assert_eq!(parse("theme dracula"), Cmd::Theme("dracula".into()));
        assert_eq!(parse("theme"), Cmd::Theme(String::new()));
    }

    #[test]
    fn parses_terminate_with_pid() {
        assert_eq!(parse("terminate 482"), Cmd::Terminate(482));
        // missing or malformed pid -> Unknown
        assert!(matches!(parse("terminate"), Cmd::Unknown(_)));
        assert!(matches!(parse("terminate abc"), Cmd::Unknown(_)));
    }

    #[test]
    fn parses_connect() {
        assert_eq!(parse("connect"), Cmd::Connect(None));
        assert_eq!(parse("connect prod"), Cmd::Connect(Some("prod".into())));
        assert_eq!(
            parse("connect postgres://u:p@h/db"),
            Cmd::Connect(Some("postgres://u:p@h/db".into()))
        );
    }

    #[test]
    fn empty_is_unknown() {
        assert_eq!(parse(""), Cmd::Unknown(String::new()));
        assert_eq!(parse("   "), Cmd::Unknown(String::new()));
    }

    #[test]
    fn suggest_first_word_prefix() {
        assert_eq!(suggest("sche"), Some("mas".to_string()));
        assert_eq!(suggest("table"), Some("s".to_string()));
    }

    #[test]
    fn suggest_returns_first_match() {
        assert_eq!(suggest("s"), Some("chemas".to_string()));
        assert_eq!(suggest("t"), Some("ables".to_string()));
        assert_eq!(suggest("q"), Some("uery".to_string()));
        assert_eq!(suggest("c"), Some("onnections".to_string()));
        assert_eq!(suggest("th"), Some("emes".to_string()));
    }

    #[test]
    fn suggest_narrows_with_more_chars() {
        assert_eq!(suggest("se"), Some("ssions".to_string()));
        assert_eq!(suggest("ta"), Some("bles".to_string()));
        assert_eq!(suggest("te"), Some("rminate".to_string()));
        assert_eq!(suggest("qui"), Some("t".to_string()));
        assert_eq!(suggest("con"), Some("nections".to_string()));
    }

    #[test]
    fn suggest_alias_prefix_of_name() {
        assert_eq!(suggest("sc"), Some("hemas".to_string()));
        assert_eq!(suggest("db"), None);
        assert_eq!(suggest("tb"), None);
        assert_eq!(suggest("sq"), None);
    }

    #[test]
    fn suggest_no_match() {
        assert_eq!(suggest("xyz"), None);
        assert_eq!(suggest("z"), None);
    }

    #[test]
    fn suggest_empty_is_none() {
        assert_eq!(suggest(""), None);
    }

    #[test]
    fn suggest_full_word_returns_empty_string() {
        assert_eq!(suggest("schemas"), Some(String::new()));
        assert_eq!(suggest("quit"), Some(String::new()));
    }

    #[test]
    fn suggest_theme_arg() {
        assert_eq!(suggest("theme dr"), Some("acula".to_string()));
        assert_eq!(suggest("theme no"), Some("rd".to_string()));
        assert_eq!(suggest("theme xyz"), None);
    }

    #[test]
    fn suggest_non_completable_arg_returns_none() {
        assert_eq!(suggest("terminate 1"), None);
        assert_eq!(suggest("connect "), None);
    }

    #[test]
    fn suggest_theme_uses_alias_too() {
        assert_eq!(suggest("theme gruv"), Some("box-dark".to_string()));
    }

    #[test]
    fn open_populates_all_commands() {
        let mut p = Palette::default();
        p.open();
        assert_eq!(p.filtered.len(), COMMANDS.len());
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn push_filters_by_prefix() {
        let mut p = Palette::default();
        p.open();
        p.push('s');
        let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
        assert!(names.contains(&"schemas"));
        assert!(names.contains(&"sessions"));
        assert!(!names.contains(&"tables"));
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn push_then_backspace_restores_all() {
        let mut p = Palette::default();
        p.open();
        p.push('s');
        assert!(p.filtered.len() < COMMANDS.len());
        p.backspace();
        assert_eq!(p.filtered.len(), COMMANDS.len());
    }

    #[test]
    fn no_match_gives_empty_filtered() {
        let mut p = Palette::default();
        p.open();
        p.push('z');
        assert!(p.filtered.is_empty());
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn space_in_buffer_still_computes_filtered() {
        let mut p = Palette::default();
        p.open();
        for c in "theme ".chars() {
            p.push(c);
        }
        let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
        assert_eq!(names, vec!["themes", "theme"]);
    }

    #[test]
    fn prefix_te_matches_terminate() {
        let mut p = Palette::default();
        p.open();
        p.push('t');
        p.push('e');
        let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
        assert!(names.contains(&"terminate"));
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn move_down_advances_selected_and_fills_buffer() {
        let mut p = Palette::default();
        p.open();
        assert_eq!(p.selected, 0);
        let first = COMMANDS[p.filtered[0]].name.to_string();
        p.move_down();
        assert_eq!(p.selected, 1);
        assert_eq!(p.buffer, COMMANDS[p.filtered[1]].name);
        assert_ne!(p.buffer, first);
    }

    #[test]
    fn move_up_wraps_to_last_and_fills_buffer() {
        let mut p = Palette::default();
        p.open();
        assert_eq!(p.selected, 0);
        p.move_up();
        assert_eq!(p.selected, COMMANDS.len() - 1);
        assert_eq!(p.buffer, COMMANDS[p.filtered[COMMANDS.len() - 1]].name);
    }

    #[test]
    fn move_down_wraps_to_zero() {
        let mut p = Palette::default();
        p.open();
        p.selected = COMMANDS.len() - 1;
        p.move_down();
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn move_does_nothing_on_empty_filtered() {
        let mut p = Palette::default();
        p.open();
        p.push('z');
        assert!(p.filtered.is_empty());
        p.move_up();
        p.move_down();
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn select_item_fills_buffer() {
        let mut p = Palette::default();
        p.open();
        p.push('t');
        p.push('e');
        let names: Vec<&str> = p.filtered.iter().map(|&i| COMMANDS[i].name).collect();
        let first_name = names[0];
        p.select_item();
        assert_eq!(p.buffer, first_name);
    }

    #[test]
    fn select_item_noop_on_empty_filtered() {
        let mut p = Palette::default();
        p.open();
        p.push('z');
        assert!(p.filtered.is_empty());
        p.select_item();
        assert_eq!(p.buffer, "z");
    }

    #[test]
    fn full_dropdown_workflow() {
        let mut p = Palette::default();
        p.open();
        assert!(p.filtered.len() > 1);

        p.push('t');
        assert!(p.filtered.len() < COMMANDS.len());
        let selected_name = COMMANDS[p.filtered[p.selected]].name.to_string();

        p.move_down();
        let next_name = COMMANDS[p.filtered[p.selected]].name.to_string();
        assert_ne!(selected_name, next_name);

        p.select_item();
        assert_eq!(p.buffer, next_name);

        p.close();
        assert!(!p.open);
        assert!(p.buffer.is_empty());
        assert!(p.filtered.is_empty());
    }
}
