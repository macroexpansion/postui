//! Sanity-check that representative views render without panicking and
//! produce sensible buffers via the TestBackend.

use postui::ui::theme;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use tokio::sync::mpsc;

#[test]
fn help_modal_renders_text() {
    use postui::views::{Ctx, Modal, help::HelpModal};
    let backend = TestBackend::new(80, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut help = HelpModal::new();
    let theme = &theme::DEFAULT;
    let (_tx, _rx) = mpsc::channel(8);
    let _ctx = Ctx::new(_tx);

    term.draw(|f| {
        let area = Rect::new(0, 0, 80, 30);
        help.render(f, area, theme);
    })
    .unwrap();

    let buf = term.backend().buffer();
    let dump = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(dump.contains("postui"));
    assert!(dump.contains("palette"));
    assert!(dump.contains(":query"));
}

#[test]
fn themes_modal_renders_table() {
    use postui::views::{Modal, themes::ThemesModal};
    let backend = TestBackend::new(80, 30);
    let mut term = Terminal::new(backend).unwrap();
    let mut modal = ThemesModal::new(&theme::DEFAULT);
    let theme = &theme::DEFAULT;

    term.draw(|f| {
        let area = Rect::new(0, 0, 80, 30);
        modal.render(f, area, theme);
    })
    .unwrap();

    let buf = term.backend().buffer();
    let dump = buf.content().iter().map(|c| c.symbol()).collect::<String>();
    assert!(dump.contains("themes"), "expected block title 'themes'");
    assert!(
        dump.contains("default"),
        "expected 'default' theme name in list"
    );
    assert!(
        dump.contains("dracula"),
        "expected 'dracula' theme name in list"
    );
}
