//! Multi-line editor wrapper around tui-textarea, with a $EDITOR shell-out.

use std::{
    io::{Read, Write, stdout},
    process::Command,
};

use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
};
use tui_textarea::TextArea;

use crate::ui::theme::Theme;

pub struct Editor {
    pub area: TextArea<'static>,
}

impl Editor {
    pub fn new() -> Self {
        let mut area = TextArea::default();
        area.set_block(Block::default().borders(Borders::ALL));
        area.set_cursor_line_style(Style::default());
        Self { area }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.area.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        self.area.set_style(Style::default().fg(theme.fg));
        f.render_widget(&self.area, area);
    }

    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }

    pub fn set_text(&mut self, s: &str) {
        self.area = TextArea::from(s.lines().map(String::from).collect::<Vec<_>>());
        self.area.set_cursor_line_style(Style::default());
    }

    pub fn clear(&mut self) {
        self.area = TextArea::default();
    }
}

impl Default for Editor {
    fn default() -> Self { Self::new() }
}

/// Suspend the TUI, open the user's $EDITOR with the given text, restore.
/// Returns the new text on success.
pub fn shell_out_to_editor(initial: &str) -> std::io::Result<String> {
    let editor = std::env::var("EDITOR").or_else(|_| std::env::var("VISUAL")).unwrap_or_else(|_| "nvim".into());

    // Write current buffer to a temp file.
    let mut tmp = tempfile::Builder::new()
        .prefix("postui-")
        .suffix(".sql")
        .tempfile()?;
    tmp.write_all(initial.as_bytes())?;
    let path = tmp.path().to_path_buf();
    drop(tmp);

    // Tear down TUI (raw mode + alt screen) so $EDITOR has a normal terminal.
    let mut out = stdout();
    let _ = out.execute(DisableMouseCapture);
    let _ = out.execute(LeaveAlternateScreen);
    let _ = disable_raw_mode();

    let status = Command::new(&editor).arg(&path).status();

    // Restore TUI regardless of editor exit status.
    let _ = enable_raw_mode();
    let _ = stdout().execute(EnterAlternateScreen);
    let _ = stdout().execute(EnableMouseCapture);

    let _ = status?;

    let mut text = String::new();
    std::fs::File::open(&path)?.read_to_string(&mut text)?;
    let _ = std::fs::remove_file(&path);
    Ok(text)
}
