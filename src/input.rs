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
    if state.show_help {
        handle_help_key(state, key);
        return;
    }
    if state.show_outline {
        handle_outline_key(state, key);
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
        KeyCode::Char('l') | KeyCode::Right => state.scroll_right(8),
        KeyCode::Char('h') | KeyCode::Left => state.scroll_left(8),
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
        // heading navigation
        KeyCode::Char('t') => state.next_heading(),
        KeyCode::Char('T') => state.prev_heading(),
        KeyCode::Char('o') => state.toggle_outline(),
        // folding
        KeyCode::Tab => state.toggle_fold(),
        // search
        KeyCode::Char('/') => state.start_search(),
        KeyCode::Char('n') => state.next_match(),
        KeyCode::Char('N') => state.prev_match(),
        // help
        KeyCode::Char('?') => state.toggle_help(),
        _ => {}
    }
}

fn handle_help_key(state: &mut PagerState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc | KeyCode::Char('?') => {
            state.toggle_help()
        }
        _ => {}
    }
}

/// While the outline overlay is showing, j/k/arrows move the selection,
/// Enter jumps, and Esc/q/o close without jumping.
fn handle_outline_key(state: &mut PagerState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => state.outline_next(),
        KeyCode::Char('k') | KeyCode::Up => state.outline_prev(),
        KeyCode::Enter => state.outline_jump(),
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Char('o') => {
            state.show_outline = false;
        }
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
            false,
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
    fn question_mark_toggles_help() {
        let mut s = state("abc");
        handle_key(&mut s, key('?'));
        assert!(s.show_help);
        handle_key(&mut s, key('?'));
        assert!(!s.show_help);
    }

    #[test]
    fn q_closes_help_without_quitting() {
        let mut s = state("abc");
        s.show_help = true;

        handle_key(&mut s, key('q'));
        assert!(!s.show_help);
        assert!(!s.quit);
    }

    #[test]
    fn escape_closes_help_without_quitting() {
        let mut s = state("abc");
        s.show_help = true;

        handle_key(&mut s, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!s.show_help);
        assert!(!s.quit);
    }

    #[test]
    fn uppercase_q_closes_help_without_quitting() {
        let mut s = state("abc");
        s.show_help = true;

        handle_key(&mut s, key('Q'));
        assert!(!s.show_help);
        assert!(!s.quit);
    }

    #[test]
    fn h_pans_left_instead_of_toggling_help() {
        let mut s = state("x");
        s.doc.lines = vec![ratatui::text::Line::raw("0123456789abcdef")];
        s.width = 5;
        s.h_offset = 8;

        handle_key(&mut s, key('h'));
        assert_eq!(s.h_offset, 0);
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
    fn left_and_right_pan_horizontally() {
        let mut s = state("x");
        s.doc.lines = vec![ratatui::text::Line::raw("0123456789abcdef")];
        s.width = 5;

        handle_key(&mut s, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(s.h_offset, 8);
        handle_key(&mut s, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(s.h_offset, 0);
    }

    #[test]
    fn h_and_l_pan_horizontally() {
        let mut s = state("x");
        s.doc.lines = vec![ratatui::text::Line::raw("0123456789abcdef")];
        s.width = 5;

        handle_key(&mut s, key('l'));
        assert_eq!(s.h_offset, 8);
        handle_key(&mut s, key('h'));
        assert_eq!(s.h_offset, 0);
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

    fn md_state(md: &str) -> PagerState {
        PagerState::new(
            Input {
                text: md.to_owned(),
                render_mode: ResolvedMode::Markdown,
                source_path: None,
            },
            2,
            80,
            false,
        )
    }

    #[test]
    fn o_toggles_outline() {
        let mut s = md_state("# A\n\n## B\n");
        handle_key(&mut s, key('o'));
        assert!(s.show_outline);
        handle_key(&mut s, key('o'));
        assert!(!s.show_outline);
    }

    #[test]
    fn t_jumps_to_next_heading() {
        let mut s = md_state("# A\n\ntext\n\n## B\n");
        assert_eq!(s.offset, 0);
        handle_key(&mut s, key('t'));
        assert_eq!(s.offset, s.doc.headings[1].line);
    }

    #[test]
    fn t_jumps_to_prev_heading() {
        let mut s = md_state("# A\n\ntext\n\n## B\n");
        s.offset = s.doc.headings[1].line;
        handle_key(&mut s, key('T'));
        assert_eq!(s.offset, s.doc.headings[0].line);
    }

    #[test]
    fn outline_j_moves_selection() {
        let mut s = md_state("# A\n\n## B\n\n## C\n");
        handle_key(&mut s, key('o'));
        assert_eq!(s.outline_selection, 0);
        handle_key(&mut s, key('j'));
        assert_eq!(s.outline_selection, 1);
        handle_key(&mut s, key('k'));
        assert_eq!(s.outline_selection, 0);
    }

    #[test]
    fn outline_enter_jumps_and_closes() {
        let mut s = md_state("# A\n\n## B\n\n## C\n");
        handle_key(&mut s, key('o'));
        handle_key(&mut s, key('j')); // select B
        handle_key(&mut s, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!s.show_outline);
        assert_eq!(s.offset, s.doc.headings[1].line);
    }

    #[test]
    fn outline_esc_closes_without_jumping() {
        let mut s = md_state("# A\n\n## B\n");
        let initial_offset = s.offset;
        handle_key(&mut s, key('o'));
        handle_key(&mut s, key('j'));
        handle_key(&mut s, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!s.show_outline);
        assert_eq!(s.offset, initial_offset);
    }

    #[test]
    fn tab_toggles_fold() {
        let mut s = md_state("# A\n\nbody line\n\n## B");
        let full = s.line_count();
        handle_key(&mut s, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(s.line_count() < full, "folding should reduce visible lines");
        handle_key(&mut s, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(s.line_count(), full);
    }
}
