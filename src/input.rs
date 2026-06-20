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
    // Digit-prefix count: digits accumulate into `pending_count`; the next
    // non-digit command consumes (or discards) it. Matches `less`.
    if let KeyCode::Char(c) = key.code
        && c.is_ascii_digit()
    {
        let d = (c as u8) - b'0';
        state.push_digit(d);
        return;
    }
    let count_before = state.pending_count;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // quit
        KeyCode::Char('q') | KeyCode::Char('Q') => state.quit(),
        KeyCode::Char('c') if ctrl => state.quit(),
        KeyCode::Esc => state.quit(),
        // repaint — must be matched before the pan bindings since 'l' with
        // Ctrl would otherwise be caught by the bare `Char('l')` arm.
        KeyCode::Char('l') if ctrl => state.repaint(),
        // movement (one line)
        KeyCode::Char('j') | KeyCode::Char('e') | KeyCode::Down => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.scroll_down(n);
        }
        KeyCode::Char('k') | KeyCode::Char('y') | KeyCode::Up => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.scroll_up(n);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(8);
            state.scroll_right(n);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(8);
            state.scroll_left(n);
        }
        KeyCode::Char('J') => state.scroll_down(1),
        KeyCode::Char('K') => state.scroll_up(1),
        // movement (one page)
        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.page_down_n(n);
        }
        KeyCode::Char('b') | KeyCode::PageUp => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.page_up_n(n);
        }
        // movement (half page, vim-style)
        KeyCode::Char('d') if ctrl => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.half_page_down_n(n);
        }
        KeyCode::Char('u') if ctrl => {
            let n = state.take_count().filter(|&n| n > 0).unwrap_or(1);
            state.half_page_up_n(n);
        }
        // jump
        KeyCode::Char('g') | KeyCode::Home => match state.take_count() {
            Some(n) => state.goto_line(n),
            None => state.goto_top(),
        },
        KeyCode::Char('G') | KeyCode::End => match state.take_count() {
            Some(n) => state.goto_line(n),
            None => state.goto_bottom(),
        },
        // `N p` / `N %` — jump to N percent (0..=100).
        KeyCode::Char('p') | KeyCode::Char('%') => match state.take_count() {
            Some(pct) => state.goto_percent(pct as u16),
            None => {
                state.status = "expected count before p/%".to_owned();
            }
        },
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
        // highlight toggles (Esc-u / Esc-U)
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::ALT) => state.toggle_highlight(),
        KeyCode::Char('U') if key.modifiers.contains(KeyModifiers::ALT) => state.clear_search(),
        // repaint (r)
        KeyCode::Char('r') => state.repaint(),
        // help
        KeyCode::Char('?') => state.toggle_help(),
        _ => {}
    }
    // Discard the count if the dispatched command didn't consume it and
    // we didn't just enter search mode (which needs the count for
    // `Nth match` semantics). Matches `less`'s "any non-digit key
    // consumes the count" behavior.
    if state.pending_count == count_before && !matches!(state.mode, Mode::Search(_)) {
        state.clear_count();
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

    // -- repaint / highlight toggles -----------------------------------------

    #[test]
    fn r_repaint_is_a_noop() {
        let mut s = state("hello");
        let initial_offset = s.offset;
        handle_key(&mut s, key('r'));
        assert_eq!(s.offset, initial_offset);
        assert!(!s.quit);
    }

    #[test]
    fn ctrl_l_repaint_is_a_noop() {
        let mut s = state("hello");
        handle_key(
            &mut s,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
        );
        assert_eq!(s.offset, 0);
        assert!(!s.quit);
    }

    #[test]
    fn alt_u_toggles_highlight() {
        use crate::pager::HighlightMode;
        let mut s = state("hello");
        assert_eq!(s.highlight, HighlightMode::All);
        handle_key(&mut s, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT));
        assert_eq!(s.highlight, HighlightMode::None);
        handle_key(&mut s, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT));
        assert_eq!(s.highlight, HighlightMode::All);
    }

    #[test]
    fn alt_uppercase_u_clears_search() {
        use crate::pager::HighlightMode;
        let mut s = state("hello\nhello");
        s.start_search();
        for c in "hello".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert!(s.search.is_some());

        handle_key(&mut s, KeyEvent::new(KeyCode::Char('U'), KeyModifiers::ALT));
        assert!(s.search.is_none());
        assert_eq!(s.highlight, HighlightMode::None);
    }

    // -- digit-prefix count --------------------------------------------------

    #[test]
    fn digit_accumulates_in_pending_count() {
        let mut s = state("hello");
        handle_key(&mut s, key('1'));
        handle_key(&mut s, key('2'));
        handle_key(&mut s, key('3'));
        assert_eq!(s.pending_count, Some(123));
    }

    #[test]
    fn zero_is_a_real_digit() {
        // `0` starts a count, useful for `0G` = top.
        let mut s = state("hello");
        handle_key(&mut s, key('0'));
        assert_eq!(s.pending_count, Some(0));
    }

    #[test]
    fn digit_then_j_scrolls_n_lines() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('5'));
        handle_key(&mut s, key('j'));
        assert_eq!(s.offset, 5);
        assert_eq!(s.pending_count, None);
    }

    #[test]
    fn digit_then_k_scrolls_up_n_lines() {
        let mut s = state(&"a\n".repeat(50));
        s.offset = 20;
        handle_key(&mut s, key('3'));
        handle_key(&mut s, key('k'));
        assert_eq!(s.offset, 17);
    }

    #[test]
    fn digit_then_space_pages_down_n_times() {
        let mut s = state(&"a\n".repeat(200));
        // height = 23, page step = 22; 2 pages = 44.
        handle_key(&mut s, key('2'));
        handle_key(&mut s, key(' '));
        assert_eq!(s.offset, 44);
    }

    #[test]
    fn digit_then_g_jumps_to_line_n() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('1'));
        handle_key(&mut s, key('0'));
        handle_key(&mut s, key('G'));
        // 1-based line 10 → index 9.
        assert_eq!(s.offset, 9);
    }

    #[test]
    fn g_without_count_goes_to_top() {
        let mut s = state(&"a\n".repeat(50));
        s.offset = 30;
        handle_key(&mut s, key('g'));
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn g_uppercase_without_count_goes_to_bottom() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('G'));
        assert_eq!(s.offset, s.max_offset());
    }

    #[test]
    fn digit_then_percent_jumps_to_n_percent() {
        let mut s = state(&"a\n".repeat(100));
        handle_key(&mut s, key('5'));
        handle_key(&mut s, key('0'));
        handle_key(&mut s, key('%'));
        // 50% of 99 = 49.
        assert_eq!(s.offset, 49);
    }

    #[test]
    fn percent_without_count_sets_error_status() {
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('%'));
        assert!(s.status.contains("expected count"));
    }

    #[test]
    fn non_counted_command_clears_pending_count() {
        // `5?` should drop the `5` since help doesn't use it.
        let mut s = state(&"a\n".repeat(50));
        handle_key(&mut s, key('5'));
        handle_key(&mut s, key('?'));
        assert!(s.show_help, "? should open help");
        assert_eq!(s.pending_count, None, "count should be discarded");
        // Close help, then verify a follow-up `j` defaults to 1.
        handle_key(&mut s, key('q'));
        assert!(!s.show_help);
        let initial = s.offset;
        handle_key(&mut s, key('j'));
        assert_eq!(s.offset, initial + 1);
    }

    #[test]
    fn digit_then_n_search_uses_count_as_nth_match() {
        let mut s = state("foo\nbar\nfoo\nbar\nfoo");
        handle_key(&mut s, key('2'));
        handle_key(&mut s, key('/'));
        for c in "foo".chars() {
            handle_key(&mut s, key(c));
        }
        handle_key(&mut s, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let search = s.search.as_ref().unwrap();
        assert_eq!(search.current, 1);
    }
}
