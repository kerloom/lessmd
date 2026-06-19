//! Key-to-action dispatch. Split from `pager.rs` so the state math stays
//! in one place and the keybinding table lives in another.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::pager::{Mode, PagerState};

/// Handle a single key press in the context of `state`.
pub fn handle_key(state: &mut PagerState, key: KeyEvent) {
    if let Mode::Search(_) = state.mode {
        handle_search_key(state, key);
        return;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // quit
        KeyCode::Char('q') | KeyCode::Char('Q') => state.quit(),
        KeyCode::Char('c') if ctrl => state.quit(),
        KeyCode::Esc => state.quit(),
        // movement (one line)
        KeyCode::Char('j') | KeyCode::Char('e') | KeyCode::Down => state.scroll_down(1),
        KeyCode::Char('k') | KeyCode::Char('y') | KeyCode::Up => state.scroll_up(1),
        KeyCode::Char('J') => state.scroll_down(1),
        KeyCode::Char('K') => state.scroll_up(1),
        // movement (one page)
        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => state.page_down(),
        KeyCode::Char('b') | KeyCode::PageUp => state.page_up(),
        // movement (half page, vim-style)
        KeyCode::Char('d') if ctrl => state.half_page_down(),
        KeyCode::Char('u') if ctrl => state.half_page_up(),
        // jump
        KeyCode::Char('g') | KeyCode::Home => state.goto_top(),
        KeyCode::Char('G') | KeyCode::End => state.goto_bottom(),
        // search
        KeyCode::Char('/') => state.start_search(),
        KeyCode::Char('n') => state.next_match(),
        KeyCode::Char('N') => state.prev_match(),
        // help
        KeyCode::Char('h') | KeyCode::Char('H') => state.toggle_help(),
        _ => {}
    }
}

/// While in search mode, typed characters build the query; Enter finalizes;
/// Esc / Ctrl-C aborts; Backspace deletes.
fn handle_search_key(state: &mut PagerState, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter => state.finalize_search(),
        KeyCode::Esc => state.cancel_search(),
        KeyCode::Char('c') if ctrl => state.cancel_search(),
        KeyCode::Backspace => state.search_backspace(),
        KeyCode::Char(c) => state.search_append(c),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Input, ResolvedMode};

    fn state(text: &str) -> PagerState {
        PagerState::new(
            Input {
                text: text.to_owned(),
                render_mode: ResolvedMode::Text { ansi: false },
                source_path: None,
            },
            24,
            80,
        )
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn q_quits() {
        let mut s = state("x");
        handle_key(&mut s, key('q'));
        assert!(s.quit);
    }

    #[test]
    fn j_scrolls_down() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('j'));
        assert_eq!(s.offset, 1);
    }

    #[test]
    fn g_then_g_jumps_top_and_bottom() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('G'));
        assert_eq!(s.offset, s.max_offset());
        handle_key(&mut s, key('g'));
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn slash_enters_search_mode() {
        let mut s = state("abc");
        handle_key(&mut s, key('/'));
        assert_eq!(s.mode, Mode::Search(String::new()));
    }

    #[test]
    fn h_toggles_help() {
        let mut s = state("abc");
        handle_key(&mut s, key('h'));
        assert!(s.show_help);
        handle_key(&mut s, key('h'));
        assert!(!s.show_help);
    }

    #[test]
    fn ctrl_d_scrolls_half_page_down() {
        let mut s = state(&"a\n".repeat(50));
        // viewport = 24 - 1 = 23; half = 23 / 2 = 11
        handle_key(
            &mut s,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(s.offset, 11);
    }

    #[test]
    fn ctrl_u_scrolls_half_page_up() {
        let mut s = state(&"a\n".repeat(50));
        s.offset = 20;
        handle_key(
            &mut s,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(s.offset, 20 - 11);
    }

    #[test]
    fn search_typing_then_enter_finalizes() {
        let mut s = state("match\nmatch\nother");
        handle_key(&mut s, key('/'));
        for c in "match".chars() {
            handle_key(&mut s, key(c));
        }
        handle_key(&mut s, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.search.as_ref().unwrap().matches.len(), 2);
    }
}
