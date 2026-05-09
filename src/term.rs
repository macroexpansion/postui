//! Terminal init/teardown. Holds RAII guard so the terminal is always restored.

use std::io::{self, Stdout, stdout};

use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// RAII handle: enters raw mode + alt screen on `init`, restores on drop.
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn init() -> io::Result<(Tui, Self)> {
        enable_raw_mode()?;
        let mut out = stdout();
        out.execute(EnterAlternateScreen)?;
        out.execute(EnableMouseCapture)?;
        let backend = CrosstermBackend::new(out);
        let term = Terminal::new(backend)?;
        Ok((term, TerminalGuard))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

/// Install a panic hook that restores the terminal before re-panicking.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = stdout();
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        tracing::error!(?info, "panic");
        prev(info);
    }));
}
