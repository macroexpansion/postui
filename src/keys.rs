//! Keymap: vim motion bindings for list/table views.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Generic motion intent emitted from a key event in a list view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    PageNext,
    PagePrev,
}

/// Stateful keymap that tracks multi-key chords (e.g. `gg` → Home).
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    pending_g: bool,
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, key: KeyEvent) -> Option<Motion> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            self.pending_g = false;
            return None;
        }

        if key.code == KeyCode::Char('g') && !self.pending_g {
            self.pending_g = true;
            return None;
        }

        let motion = match key.code {
            KeyCode::Char('g') => Motion::Home,
            KeyCode::Char('G') => Motion::End,
            KeyCode::Char('j') | KeyCode::Down => Motion::Down,
            KeyCode::Char('k') | KeyCode::Up => Motion::Up,
            KeyCode::Char('h') | KeyCode::Left => Motion::Left,
            KeyCode::Char('l') | KeyCode::Right => Motion::Right,
            KeyCode::Char('w') | KeyCode::PageDown => Motion::PageDown,
            KeyCode::Char('b') | KeyCode::PageUp => Motion::PageUp,
            KeyCode::End => Motion::End,
            KeyCode::Home => Motion::Home,
            _ => {
                self.pending_g = false;
                return None;
            }
        };

        self.pending_g = false;
        Some(motion)
    }

    pub fn is_pending(&self) -> bool {
        self.pending_g
    }
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
        let mut km = Keymap::new();
        assert_eq!(km.handle(k('j')), Some(Motion::Down));
        assert_eq!(km.handle(k('k')), Some(Motion::Up));
        assert_eq!(km.handle(k('h')), Some(Motion::Left));
        assert_eq!(km.handle(k('l')), Some(Motion::Right));
        assert_eq!(km.handle(k('w')), Some(Motion::PageDown));
        assert_eq!(km.handle(k('b')), Some(Motion::PageUp));
    }

    #[test]
    fn ctrl_modified_keys_pass_through() {
        let mut km = Keymap::new();
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(km.handle(key), None);
    }

    #[test]
    fn arrow_keys_map_too() {
        let mut km = Keymap::new();
        assert_eq!(
            km.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Motion::Down)
        );
    }

    #[test]
    fn unknown_key_is_none() {
        let mut km = Keymap::new();
        assert_eq!(km.handle(k('x')), None);
    }

    #[test]
    fn gg_produces_home() {
        let mut km = Keymap::new();
        assert_eq!(km.handle(k('g')), None);
        assert!(km.is_pending());
        assert_eq!(km.handle(k('g')), Some(Motion::Home));
        assert!(!km.is_pending());
    }

    #[test]
    fn capital_g_produces_end() {
        let mut km = Keymap::new();
        assert_eq!(km.handle(k('G')), Some(Motion::End));
        assert!(!km.is_pending());
    }

    #[test]
    fn g_then_other_key_clears_pending() {
        let mut km = Keymap::new();
        assert_eq!(km.handle(k('g')), None);
        assert!(km.is_pending());
        assert_eq!(km.handle(k('j')), Some(Motion::Down));
        assert!(!km.is_pending());
    }

    #[test]
    fn g_then_unknown_clears_pending() {
        let mut km = Keymap::new();
        km.handle(k('g'));
        assert!(km.is_pending());
        assert_eq!(km.handle(k('x')), None);
        assert!(!km.is_pending());
    }
}
