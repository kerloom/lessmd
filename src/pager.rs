//! `PagerState` — the pure, terminal-I/O-free pager state machine.
//!
//! All scrolling/searching/resize math lives here so it can be unit-tested
//! without a terminal. `main.rs` drives the event loop and draws from
//! [`PagerState::visible_lines`].

use ratatui::text::Line;

use crate::document::Document;
use crate::search::{SearchState, search_lines};
use crate::source::Input;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    /// Building a search query.
    Search(String),
}

#[derive(Debug, Clone)]
pub struct PagerState {
    /// Kept so we can re-render on width change.
    pub input: Input,
    pub doc: Document,
    /// Index of the first visible line.
    pub offset: usize,
    /// Visible rows (terminal height minus the status bar).
    pub height: usize,
    pub width: u16,
    pub mode: Mode,
    pub search: Option<SearchState>,
    pub quit: bool,
    pub show_help: bool,
    pub status: String,
}

impl PagerState {
    /// `height` is the *total* terminal height; one row is reserved for the
    /// status bar, so the viewport is `height - 1`.
    /// `height` is the *total* terminal height; one row is reserved for the
    /// status bar, so the viewport is `height - 1`. Content wraps to
    /// `width - 1` to leave a safety margin — some terminals wrap a line
    /// that exactly fills the column width onto the next physical row.
    pub fn new(input: Input, height: u16, width: u16) -> Self {
        let viewport = height.saturating_sub(1).max(1) as usize;
        let wrap_width = width.saturating_sub(1).max(1);
        let doc = Document::new(&input, wrap_width);
        Self {
            input,
            doc,
            offset: 0,
            height: viewport,
            width: wrap_width,
            mode: Mode::Normal,
            search: None,
            quit: false,
            show_help: false,
            status: String::new(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    /// Largest valid `offset` (the document fits when the last window starts
    /// here). Zero when the document is shorter than the viewport.
    pub fn max_offset(&self) -> usize {
        self.doc.line_count().saturating_sub(self.height)
    }

    /// Returns true when the viewport jumped by more than one line since
    /// `prev_offset` (e.g. Ctrl-D, Ctrl-U, PageUp/Down, g, G). The caller
    /// should force a full terminal redraw in that case to prevent stale
    /// content from surviving ratatui's diff optimization.
    pub fn jumped(&self, prev_offset: usize) -> bool {
        self.offset.abs_diff(prev_offset) > 1
    }

    pub fn visible_lines(&self) -> &[Line<'static>] {
        self.doc.slice(self.offset, self.height)
    }

    // -- scrolling -----------------------------------------------------------

    pub fn scroll_down(&mut self, n: usize) {
        self.offset = (self.offset + n).min(self.max_offset());
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.height.saturating_sub(1).max(1));
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.height.saturating_sub(1).max(1));
    }

    /// Vim-style half-page down (Ctrl-D).
    pub fn half_page_down(&mut self) {
        self.scroll_down(self.height / 2);
    }

    /// Vim-style half-page up (Ctrl-U).
    pub fn half_page_up(&mut self) {
        self.scroll_up(self.height / 2);
    }

    pub fn goto_top(&mut self) {
        self.offset = 0;
    }

    pub fn goto_bottom(&mut self) {
        self.offset = self.max_offset();
    }

    // -- resize --------------------------------------------------------------

    pub fn resize(&mut self, height: u16, width: u16) {
        self.height = height.saturating_sub(1).max(1) as usize;
        let wrap_width = width.saturating_sub(1).max(1);
        if wrap_width != self.width {
            self.width = wrap_width;
            self.doc = Document::new(&self.input, wrap_width);
            // Re-run any active search against the re-wrapped lines.
            if let Some(s) = &self.search {
                let query = s.query.clone();
                let matches = search_lines(&self.doc.lines, &query);
                let current = if matches.is_empty() {
                    0
                } else {
                    s.current.min(matches.len() - 1)
                };
                self.search = Some(SearchState {
                    query,
                    matches,
                    current,
                });
            }
        }
        self.offset = self.offset.min(self.max_offset());
    }

    // -- search --------------------------------------------------------------

    pub fn start_search(&mut self) {
        self.mode = Mode::Search(String::new());
    }

    pub fn cancel_search(&mut self) {
        self.mode = Mode::Normal;
        self.status.clear();
    }

    pub fn search_backspace(&mut self) {
        if let Mode::Search(ref mut q) = self.mode {
            q.pop();
        }
    }

    pub fn search_append(&mut self, c: char) {
        if let Mode::Search(ref mut q) = self.mode {
            q.push(c);
        }
    }

    pub fn finalize_search(&mut self) {
        let query = match &self.mode {
            Mode::Search(q) => q.clone(),
            Mode::Normal => return,
        };
        self.mode = Mode::Normal;
        let matches = search_lines(&self.doc.lines, &query);
        if matches.is_empty() {
            self.search = None;
            self.status = format!("no matches for {query:?}");
        } else {
            let current = 0;
            self.offset = matches[current].min(self.max_offset());
            self.status = format!("{}/{} matches", current + 1, matches.len());
            self.search = Some(SearchState {
                query,
                matches,
                current,
            });
        }
    }

    pub fn next_match(&mut self) {
        let Some(s) = &mut self.search else {
            return;
        };
        if s.matches.is_empty() {
            return;
        }
        s.current = (s.current + 1) % s.matches.len();
        let line = s.matches[s.current];
        let cur = s.current;
        let total = s.matches.len();
        self.offset = line.min(self.max_offset());
        self.status = format!("{}/{} matches", cur + 1, total);
    }

    pub fn prev_match(&mut self) {
        let Some(s) = &mut self.search else {
            return;
        };
        if s.matches.is_empty() {
            return;
        }
        s.current = (s.current + s.matches.len() - 1) % s.matches.len();
        let line = s.matches[s.current];
        let cur = s.current;
        let total = s.matches.len();
        self.offset = line.min(self.max_offset());
        self.status = format!("{}/{} matches", cur + 1, total);
    }

    // -- misc ----------------------------------------------------------------

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ResolvedMode;

    fn make_state(text: &str, total_height: u16, width: u16) -> PagerState {
        let input = Input {
            text: text.to_owned(),
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        };
        PagerState::new(input, total_height, width)
    }

    fn doc_with_n_lines(n: usize) -> PagerState {
        let text: Vec<String> = (0..n).map(|i| format!("line {i}")).collect();
        make_state(&text.join("\n"), 10, 80)
    }

    #[test]
    fn scroll_down_stops_at_max_offset() {
        let mut s = doc_with_n_lines(50);
        // viewport = 10 - 1 = 9; max_offset = 50 - 9 = 41
        s.scroll_down(100);
        assert_eq!(s.offset, 41);
    }

    #[test]
    fn scroll_up_stops_at_zero() {
        let mut s = doc_with_n_lines(50);
        s.offset = 5;
        s.scroll_up(100);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn page_down_moves_by_viewport_minus_one() {
        let mut s = doc_with_n_lines(50);
        s.page_down();
        // page = height - 1 = 8
        assert_eq!(s.offset, 8);
    }

    #[test]
    fn half_page_down_moves_by_half_viewport() {
        let mut s = doc_with_n_lines(50);
        // viewport = 9; half = 9 / 2 = 4
        s.half_page_down();
        assert_eq!(s.offset, 4);
    }

    #[test]
    fn half_page_up_moves_by_half_viewport() {
        let mut s = doc_with_n_lines(50);
        s.offset = 10;
        s.half_page_up();
        // half = 9 / 2 = 4
        assert_eq!(s.offset, 6);
    }

    #[test]
    fn half_page_down_clamps_at_max_offset() {
        let mut s = doc_with_n_lines(50);
        s.half_page_down();
        s.half_page_down();
        s.half_page_down();
        s.half_page_down();
        // 4 * 4 = 16, well under max_offset 41; keep going
        for _ in 0..20 {
            s.half_page_down();
        }
        assert_eq!(s.offset, s.max_offset());
    }

    #[test]
    fn goto_top_and_bottom() {
        let mut s = doc_with_n_lines(50);
        s.goto_bottom();
        assert_eq!(s.offset, 41);
        s.goto_top();
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn resize_clamps_offset_into_range() {
        let mut s = doc_with_n_lines(50);
        s.offset = 45; // out of range for viewport 9 (max 41)
        s.resize(10, 80);
        assert_eq!(s.offset, s.max_offset());
        assert_eq!(s.offset, 41);
    }

    #[test]
    fn resize_width_rerenders_with_new_wrap() {
        let mut s = make_state("aaaaaaaaaaaa", 20, 80); // 12 chars, no wrap at 79
        assert_eq!(s.line_count(), 1);
        s.resize(20, 5); // wrap to 5-1=4 -> 3 lines
        assert_eq!(s.line_count(), 3);
        assert_eq!(s.width, 4);
    }

    #[test]
    fn search_next_prev_wraps_around() {
        let text = "match\nmatch\nother\nmatch";
        // viewport = 3 - 1 = 2; max_offset = 4 - 2 = 2 (so line 3 clamps to 2)
        let mut s = make_state(text, 3, 80);
        s.start_search();
        for c in "match".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        // matches at lines 0, 1, 3
        assert_eq!(s.search.as_ref().unwrap().matches, vec![0, 1, 3]);
        assert_eq!(s.offset, 0);

        s.next_match(); // -> match 1 -> offset 1
        assert_eq!(s.offset, 1);
        s.next_match(); // -> match 3 -> offset clamped to max_offset 2
        assert_eq!(s.offset, 2);
        s.next_match(); // wrap -> match 0 -> offset 0
        assert_eq!(s.offset, 0);
        s.prev_match(); // wrap -> match 3 -> offset 2
        assert_eq!(s.offset, 2);
    }

    #[test]
    fn search_no_matches_sets_status() {
        let mut s = make_state("hello\nworld", 10, 80);
        s.start_search();
        for c in "zzz".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert!(s.search.is_none());
        assert!(s.status.contains("no matches"));
    }

    #[test]
    fn empty_doc() {
        let s = make_state("", 10, 80);
        assert_eq!(s.line_count(), 0);
        assert_eq!(s.max_offset(), 0);
        assert_eq!(s.offset, 0);
        assert!(s.visible_lines().is_empty());
    }

    #[test]
    fn single_line_doc() {
        let s = make_state("hello", 10, 80);
        assert_eq!(s.line_count(), 1);
        assert_eq!(s.max_offset(), 0);
    }

    #[test]
    fn doc_shorter_than_viewport_cannot_scroll() {
        let s = make_state("a\nb\nc", 10, 80);
        assert_eq!(s.line_count(), 3);
        assert_eq!(s.max_offset(), 0); // 3 < 9 viewport
        let mut s = s;
        s.scroll_down(5);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn cancel_search_returns_to_normal() {
        let mut s = make_state("abc", 10, 80);
        s.start_search();
        s.search_append('x');
        s.cancel_search();
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn search_backspace_pops_query() {
        let mut s = make_state("abc", 10, 80);
        s.start_search();
        s.search_append('x');
        s.search_append('y');
        s.search_backspace();
        match s.mode {
            Mode::Search(ref q) => assert_eq!(q, "x"),
            _ => panic!("expected search mode"),
        }
    }

    // -- regression tests for the stale-render bug --------------------------

    #[test]
    fn jumped_true_for_multi_line_scroll() {
        let mut s = doc_with_n_lines(50);
        s.offset = 10;
        // half-page jump
        assert!(s.jumped(0));
        assert!(s.jumped(20));
    }

    #[test]
    fn jumped_false_for_single_line_scroll() {
        let mut s = doc_with_n_lines(50);
        s.offset = 1;
        assert!(!s.jumped(0));
        assert!(!s.jumped(2));
        // no movement
        s.offset = 5;
        assert!(!s.jumped(5));
    }

    #[test]
    fn wrap_width_is_one_less_than_terminal() {
        // Safety margin prevents terminal-emulator line wrapping, which
        // caused "random letters" to appear after scrolling.
        let s = make_state("x", 24, 80);
        assert_eq!(s.width, 79);
    }

    #[test]
    fn resize_uses_wrap_width_minus_one() {
        let mut s = make_state("x", 24, 80);
        s.resize(24, 100);
        assert_eq!(s.width, 99);
    }
}
