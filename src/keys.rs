//! Keymap: vim motion bindings for list/table views.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Generic motion intent emitted from a key event in a list view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
    PageUp, // w / PageUp / Ctrl-D — but we use w for forward, b for back
    PageDown,
    Home,     // gg
    End,      // e (jump to last row, per spec)
    PageNext, // PageDown
    PagePrev, // PageUp
}

pub fn vim_motion(key: KeyEvent) -> Option<Motion> {
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    Some(match key.code {
        KeyCode::Char('j') | KeyCode::Down => Motion::Down,
        KeyCode::Char('k') | KeyCode::Up => Motion::Up,
        KeyCode::Char('h') | KeyCode::Left => Motion::Left,
        KeyCode::Char('l') | KeyCode::Right => Motion::Right,
        KeyCode::Char('w') | KeyCode::PageDown => Motion::PageDown,
        KeyCode::Char('b') | KeyCode::PageUp => Motion::PageUp,
        KeyCode::Char('e') | KeyCode::End => Motion::End,
        KeyCode::Home => Motion::Home,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn vim_keys_map_to_motions() {
        assert_eq!(vim_motion(k('j')), Some(Motion::Down));
        assert_eq!(vim_motion(k('k')), Some(Motion::Up));
        assert_eq!(vim_motion(k('h')), Some(Motion::Left));
        assert_eq!(vim_motion(k('l')), Some(Motion::Right));
        assert_eq!(vim_motion(k('w')), Some(Motion::PageDown));
        assert_eq!(vim_motion(k('b')), Some(Motion::PageUp));
        assert_eq!(vim_motion(k('e')), Some(Motion::End));
    }

    #[test]
    fn ctrl_modified_keys_pass_through() {
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(vim_motion(key), None);
    }

    #[test]
    fn arrow_keys_map_too() {
        assert_eq!(
            vim_motion(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Motion::Down)
        );
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(vim_motion(k('x')), None);
    }
}
