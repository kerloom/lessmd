//! `PagerState` — the pure, terminal-I/O-free pager state machine.
//!
//! All scrolling/searching/resize math lives here so it can be unit-tested
//! without a terminal. `main.rs` drives the event loop and draws from
//! [`PagerState::visible_lines`].

use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::HashSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::document::Document;
use crate::render::{RenderOptions, TableMode};
use crate::search::{CaseMode, SearchDirection, SearchState, search_lines};
use crate::source::Input;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    /// Building a search query.
    Search {
        query: String,
        direction: SearchDirection,
    },
}

/// How search matches are highlighted on screen. Mirrors `less`'s `-g` and
/// `-G` flags; `ESC-u` toggles between `All` and `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HighlightMode {
    /// Highlight every match (default).
    #[default]
    All,
    /// Highlight only the current match (`-g`).
    Last,
    /// Suppress all match highlighting (`-G`).
    None,
}

/// Mirrors `less`'s `-e` / `-E` flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QuitAtEof {
    #[default]
    Never,
    /// `-e` / `--quit-at-eof`: exit on the second EOF attempt.
    SecondAttempt,
    /// `-E` / `--QUIT-AT-EOF`: exit on the first EOF attempt.
    FirstAttempt,
}

#[derive(Debug, Clone)]
pub struct PagerState {
    /// Kept so we can re-render on width change.
    pub input: Input,
    pub render_options: RenderOptions,
    pub doc: Document,
    /// Index of the first visible line.
    pub offset: usize,
    /// First visible terminal cell within each rendered line.
    pub h_offset: usize,
    /// Visible rows (terminal height minus the status bar).
    pub height: usize,
    pub width: u16,
    pub mode: Mode,
    pub search: Option<SearchState>,
    pub quit: bool,
    pub show_help: bool,
    pub show_outline: bool,
    pub outline_selection: usize,
    pub line_numbers: bool,
    /// Case-sensitivity mode for `/` searches. Mirrors `less`'s `-i`/`-I`.
    pub case_mode: CaseMode,
    /// Search-match highlight mode. Mirrors `less`'s `-g`/`-G` and the
    /// `ESC-u` / `ESC-U` runtime toggles.
    pub highlight: HighlightMode,
    /// Pending digit prefix for the next command. Typing `1` then `0` then
    /// `j` sets this to `Some(10)` and the next `j` scrolls 10 lines.
    /// Matches `less`'s "N may precede a command" behavior.
    pub pending_count: Option<usize>,
    /// Heading indices (into `doc.headings`) whose body is folded.
    pub folded: HashSet<usize>,
    /// Maps each visible row → index into `doc.lines`. Rebuilt when folds
    /// change or the document is re-rendered.
    pub visible_indices: Vec<usize>,
    /// Enhanced lines for the initial viewport. Used by the two-phase UI to
    /// show syntax/Mermaid in the first screen before the full enhanced render
    /// completes. Only applies while `offset == 0`.
    pub viewport_overlay: Option<Vec<Line<'static>>>,
    /// Force the next draw to clear the terminal first. Used when content is
    /// re-rendered in place and may shrink, leaving stale cells otherwise.
    force_redraw: bool,
    pub status: String,
    /// Milliseconds until an ephemeral status message auto-clears. Zero when
    /// idle or after dismissal.
    status_ttl_ms: u32,
    /// Whether the current status message is an error (drawn in red) rather
    /// than a normal hint.
    status_is_error: bool,
    /// `-e` / `-E`: quit when the user tries to scroll past EOF.
    pub quit_at_eof: QuitAtEof,
    /// `-q` / `-Q`: suppress the terminal bell (no-op until a bell exists).
    pub quiet: bool,
    eof_attempts: u8,
}

/// How long ephemeral status messages stay visible without scrolling.
pub const STATUS_MESSAGE_TTL_MS: u32 = 3000;

impl PagerState {
    /// `height` is the *total* terminal height; one row is reserved for the
    /// status bar, so the viewport is `height - 1`. Content wraps to
    /// `width - 1` to leave a safety margin — some terminals wrap a line
    /// that exactly fills the column width onto the next physical row.
    /// When `line_numbers` is true, the wrap width is further narrowed by
    /// the gutter width so line numbers don't reduce the visible content.
    pub fn new(input: Input, height: u16, width: u16, line_numbers: bool) -> Self {
        Self::new_with_options(input, height, width, line_numbers, RenderOptions::default())
    }

    pub fn new_with_options(
        input: Input,
        height: u16,
        width: u16,
        line_numbers: bool,
        render_options: RenderOptions,
    ) -> Self {
        let viewport = height.saturating_sub(1).max(1) as usize;
        let wrap_width = width.saturating_sub(1).max(1);
        let doc = render_doc(&input, wrap_width, line_numbers, render_options);
        let content_width = if line_numbers {
            wrap_width.saturating_sub(gutter_width(doc.line_count()) as u16)
        } else {
            wrap_width
        };
        let mut state = Self {
            input,
            render_options,
            doc,
            offset: 0,
            h_offset: 0,
            height: viewport,
            width: content_width as u16,
            mode: Mode::Normal,
            search: None,
            quit: false,
            show_help: false,
            show_outline: false,
            outline_selection: 0,
            line_numbers,
            case_mode: CaseMode::default(),
            highlight: HighlightMode::default(),
            pending_count: None,
            folded: HashSet::new(),
            visible_indices: Vec::new(),
            viewport_overlay: None,
            force_redraw: false,
            status: String::new(),
            status_ttl_ms: 0,
            status_is_error: false,
            quit_at_eof: QuitAtEof::default(),
            quiet: false,
            eof_attempts: 0,
        };
        state.rebuild_visible_indices();
        state
    }

    pub fn replace_doc(&mut self, doc: Document, render_options: RenderOptions) {
        let mermaid_failures = doc.mermaid_failures;
        self.doc = doc;
        self.render_options = render_options;
        self.viewport_overlay = None;
        if mermaid_failures > 0 {
            self.set_error_message(mermaid_failure_message(mermaid_failures));
        } else {
            self.clear_status_message();
        }
        self.folded.clear();
        self.outline_selection = self
            .outline_selection
            .min(self.doc.headings.len().saturating_sub(1));
        if let Some(s) = &self.search {
            let query = s.query.clone();
            let matches = search_lines(&self.doc.lines, &query, self.case_mode);
            let current = if matches.is_empty() {
                0
            } else {
                s.current.min(matches.len() - 1)
            };
            self.search = Some(SearchState {
                query,
                direction: s.direction,
                matches,
                current,
            });
        }
        self.rebuild_visible_indices();
        self.offset = self.offset.min(self.max_offset());
        self.h_offset = self.h_offset.min(self.max_h_offset());
    }

    pub fn line_count(&self) -> usize {
        self.visible_indices.len()
    }

    /// Largest valid `offset` (the document fits when the last window starts
    /// here). Zero when the document is shorter than the viewport.
    pub fn max_offset(&self) -> usize {
        self.visible_indices.len().saturating_sub(self.height)
    }

    pub fn max_h_offset(&self) -> usize {
        self.max_line_width().saturating_sub(self.width as usize)
    }

    /// Width of the line-number gutter (digits + 1 space). Zero when line
    /// numbers are off.
    pub fn gutter_width(&self) -> usize {
        if self.line_numbers {
            gutter_width(self.doc.lines.len())
        } else {
            0
        }
    }

    pub fn max_line_width(&self) -> usize {
        self.visible_indices
            .iter()
            .map(|&i| line_width(&self.doc.lines[i]))
            .max()
            .unwrap_or(0)
    }

    /// Show a temporary status message (search feedback, heading label, etc.).
    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.set_status(msg, false);
    }

    /// Like [`set_status_message`] but drawn in red.
    pub fn set_error_message(&mut self, msg: impl Into<String>) {
        self.set_status(msg, true);
    }

    fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status = msg.into();
        self.status_ttl_ms = STATUS_MESSAGE_TTL_MS;
        self.status_is_error = is_error;
    }

    /// Whether the current status message should be rendered as an error.
    pub fn status_is_error(&self) -> bool {
        self.status_is_error
    }

    pub fn clear_status_message(&mut self) {
        self.status.clear();
        self.status_ttl_ms = 0;
        self.status_is_error = false;
    }

    /// Age ephemeral status messages; called from the main event loop.
    /// Returns `true` when the status line changed and the UI should redraw.
    pub fn tick_status(&mut self, delta_ms: u32) -> bool {
        if self.status_ttl_ms == 0 {
            return false;
        }
        self.status_ttl_ms = self.status_ttl_ms.saturating_sub(delta_ms);
        if self.status_ttl_ms == 0 {
            self.status.clear();
            self.status_is_error = false;
            return true;
        }
        false
    }

    fn dismiss_status_on_movement(&mut self) {
        if !self.status.is_empty() {
            self.clear_status_message();
        }
    }

    /// Returns true when the viewport jumped by more than one line since
    /// `prev_offset` (e.g. Ctrl-D, Ctrl-U, PageUp/Down, g, G). The caller
    /// should force a full terminal redraw in that case to prevent stale
    /// content from surviving ratatui's diff optimization.
    pub fn jumped(&self, prev_offset: usize) -> bool {
        self.offset.abs_diff(prev_offset) > 1
    }

    pub fn take_force_redraw(&mut self) -> bool {
        let force = self.force_redraw;
        self.force_redraw = false;
        force
    }

    pub fn visible_lines(&self) -> Vec<&Line<'static>> {
        let start = self.offset;
        let end = (start + self.height).min(self.visible_indices.len());
        self.visible_indices[start..end]
            .iter()
            .map(|&i| &self.doc.lines[i])
            .collect()
    }

    pub fn visible_lines_panned(&self) -> Vec<Line<'static>> {
        if self.offset == 0
            && let Some(lines) = &self.viewport_overlay
        {
            return lines
                .iter()
                .take(self.height)
                .map(|line| clip_line(line, self.h_offset, self.width as usize))
                .collect();
        }
        self.visible_lines()
            .iter()
            .map(|line| clip_line(line, self.h_offset, self.width as usize))
            .collect()
    }

    pub fn set_viewport_overlay(&mut self, lines: Vec<Line<'static>>) {
        self.viewport_overlay = Some(lines);
    }

    // -- scrolling -----------------------------------------------------------

    fn at_eof(&self) -> bool {
        self.offset >= self.max_offset()
    }

    fn note_forward_scroll(&mut self, before: usize) {
        if before == self.offset && self.at_eof() {
            self.on_eof_attempt();
        } else {
            self.eof_attempts = 0;
        }
    }

    fn on_eof_attempt(&mut self) {
        match self.quit_at_eof {
            QuitAtEof::Never => {}
            QuitAtEof::SecondAttempt => {
                self.eof_attempts = self.eof_attempts.saturating_add(1);
                if self.eof_attempts >= 2 {
                    self.quit = true;
                }
            }
            QuitAtEof::FirstAttempt => self.quit = true,
        }
        let _ = self.quiet;
    }

    pub fn scroll_down(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let before = self.offset;
        self.offset = (self.offset + n).min(self.max_offset());
        self.note_forward_scroll(before);
    }

    pub fn scroll_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        self.offset = self.offset.saturating_sub(n);
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.height.saturating_sub(1).max(1));
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.height.saturating_sub(1).max(1));
    }

    /// `N f` / `N Space` — page down N times.
    pub fn page_down_n(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let before = self.offset;
        let step = self.height.saturating_sub(1).max(1);
        self.offset = self
            .offset
            .saturating_add(step.saturating_mul(n))
            .min(self.max_offset());
        self.note_forward_scroll(before);
    }

    /// `N b` — page up N times.
    pub fn page_up_n(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let step = self.height.saturating_sub(1).max(1);
        self.offset = self.offset.saturating_sub(step.saturating_mul(n));
    }

    /// Vim-style half-page down (Ctrl-D).
    pub fn half_page_down(&mut self) {
        self.scroll_down(self.height / 2);
    }

    /// Vim-style half-page up (Ctrl-U).
    pub fn half_page_up(&mut self) {
        self.scroll_up(self.height / 2);
    }

    /// `N Ctrl-D` — half-page down N times.
    pub fn half_page_down_n(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let before = self.offset;
        let step = self.height / 2;
        self.offset = self
            .offset
            .saturating_add(step.saturating_mul(n))
            .min(self.max_offset());
        self.note_forward_scroll(before);
    }

    /// `N Ctrl-U` — half-page up N times.
    pub fn half_page_up_n(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let step = self.height / 2;
        self.offset = self.offset.saturating_sub(step.saturating_mul(n));
    }

    /// `N g` / `N G` — go to line `N` (1-based). `N = 0` jumps to top.
    /// Matches `less`'s `g`/`G` behavior when prefixed with a count.
    pub fn goto_line(&mut self, n: usize) {
        if n == 0 {
            self.goto_top();
            return;
        }
        let target = n.saturating_sub(1);
        let max = self.doc.lines.len().saturating_sub(1);
        self.jump_to_doc_line(target.min(max));
    }

    /// `N p` / `N %` — go to N percent into the document (0..=100).
    pub fn goto_percent(&mut self, n: u16) {
        if self.doc.lines.is_empty() {
            return;
        }
        let pct = (n.min(100) as f64) / 100.0;
        let max_line = self.doc.lines.len() - 1;
        let line = ((max_line as f64 * pct) as usize).min(max_line);
        self.jump_to_doc_line(line);
    }

    pub fn goto_top(&mut self) {
        self.dismiss_status_on_movement();
        self.offset = 0;
    }

    pub fn goto_bottom(&mut self) {
        self.dismiss_status_on_movement();
        let before = self.offset;
        self.offset = self.max_offset();
        self.note_forward_scroll(before);
    }

    pub fn scroll_right(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let before = self.h_offset;
        self.h_offset = (self.h_offset + n).min(self.max_h_offset());
        if self.h_offset != before {
            self.force_redraw = true;
        }
    }

    pub fn scroll_left(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.dismiss_status_on_movement();
        let before = self.h_offset;
        self.h_offset = self.h_offset.saturating_sub(n);
        if self.h_offset != before {
            self.force_redraw = true;
        }
    }

    pub fn toggle_table_mode(&mut self) {
        self.render_options.table_mode = match self.render_options.table_mode {
            TableMode::Truncate => TableMode::Expand,
            TableMode::Expand => TableMode::Truncate,
        };
        let wrap_width = self.current_wrap_width();
        let doc = render_doc(
            &self.input,
            wrap_width,
            self.line_numbers,
            self.render_options,
        );
        self.width = content_width_for_doc(wrap_width, self.line_numbers, &doc) as u16;
        self.replace_doc(doc, self.render_options);
        self.force_redraw = true;
        self.set_status_message(match self.render_options.table_mode {
            TableMode::Truncate => "tables: truncate",
            TableMode::Expand => "tables: expand",
        });
    }

    // -- resize --------------------------------------------------------------

    pub fn resize(&mut self, height: u16, width: u16) {
        self.viewport_overlay = None;
        self.height = height.saturating_sub(1).max(1) as usize;
        let wrap_width = width.saturating_sub(1).max(1);
        let content_width = if self.line_numbers {
            // Probe to compute the gutter width at the new wrap width.
            let probe = Document::new_with_options(&self.input, wrap_width, self.render_options);
            wrap_width.saturating_sub(gutter_width(probe.line_count()) as u16)
        } else {
            wrap_width
        };
        if content_width != self.width {
            self.width = content_width as u16;
            self.doc = render_doc(
                &self.input,
                wrap_width,
                self.line_numbers,
                self.render_options,
            );
            // Re-run any active search against the re-wrapped lines.
            if let Some(s) = &self.search {
                let query = s.query.clone();
                let matches = search_lines(&self.doc.lines, &query, self.case_mode);
                let current = if matches.is_empty() {
                    0
                } else {
                    s.current.min(matches.len() - 1)
                };
                self.search = Some(SearchState {
                    query,
                    direction: s.direction,
                    matches,
                    current,
                });
            }
        }
        self.rebuild_visible_indices();
        self.h_offset = self.h_offset.min(self.max_h_offset());
    }

    fn current_wrap_width(&self) -> u16 {
        if self.line_numbers {
            self.width.saturating_add(self.gutter_width() as u16).max(1)
        } else {
            self.width.max(1)
        }
    }

    // -- search --------------------------------------------------------------

    pub fn start_search(&mut self) {
        self.start_search_with_direction(SearchDirection::Forward);
    }

    pub fn start_backward_search(&mut self) {
        self.start_search_with_direction(SearchDirection::Backward);
    }

    pub fn start_search_with_direction(&mut self, direction: SearchDirection) {
        self.mode = Mode::Search {
            query: String::new(),
            direction,
        };
    }

    pub fn cancel_search(&mut self) {
        self.mode = Mode::Normal;
        self.clear_status_message();
    }

    pub fn search_backspace(&mut self) {
        if let Mode::Search { ref mut query, .. } = self.mode {
            query.pop();
        }
    }

    pub fn search_append(&mut self, c: char) {
        if let Mode::Search { ref mut query, .. } = self.mode {
            query.push(c);
        }
    }

    pub fn apply_search(&mut self, query: String, direction: SearchDirection) {
        let matches = search_lines(&self.doc.lines, &query, self.case_mode);
        if matches.is_empty() {
            self.search = None;
            self.pending_count = None;
            self.set_status_message(format!("no matches for {query:?}"));
        } else {
            let n = self.pending_count.take().unwrap_or(1).max(1);
            let current = self.match_index_for_direction(&matches, direction, n);
            self.jump_to_doc_line(matches[current]);
            self.set_status_message(format!("{}/{} matches", current + 1, matches.len()));
            self.search = Some(SearchState {
                query,
                direction,
                matches,
                current,
            });
        }
    }

    pub fn finalize_search(&mut self) {
        let (query, direction) = match &self.mode {
            Mode::Search { query, direction } => (query.clone(), *direction),
            Mode::Normal => return,
        };
        self.mode = Mode::Normal;
        self.apply_search(query, direction);
    }

    pub fn next_match(&mut self) {
        let direction = self
            .search
            .as_ref()
            .map(|s| s.direction)
            .unwrap_or(SearchDirection::Forward);
        match direction {
            SearchDirection::Forward => self.advance_match(1),
            SearchDirection::Backward => self.advance_match(-1),
        }
    }

    pub fn prev_match(&mut self) {
        let direction = self
            .search
            .as_ref()
            .map(|s| s.direction)
            .unwrap_or(SearchDirection::Forward);
        match direction {
            SearchDirection::Forward => self.advance_match(-1),
            SearchDirection::Backward => self.advance_match(1),
        }
    }

    fn advance_match(&mut self, step: isize) {
        let Some(s) = &mut self.search else {
            return;
        };
        if s.matches.is_empty() {
            return;
        }
        let len = s.matches.len();
        s.current = if step.is_negative() {
            (s.current + len - (step.unsigned_abs() % len)) % len
        } else {
            (s.current + step as usize) % len
        };
        let line = s.matches[s.current];
        let cur = s.current;
        let total = len;
        self.jump_to_doc_line(line);
        self.set_status_message(format!("{}/{} matches", cur + 1, total));
    }

    fn match_index_for_direction(
        &self,
        matches: &[usize],
        direction: SearchDirection,
        n: usize,
    ) -> usize {
        let len = matches.len();
        match direction {
            SearchDirection::Forward => {
                let start = matches.partition_point(|&line| line < self.offset);
                (start + n - 1) % len
            }
            SearchDirection::Backward => {
                let before = matches.partition_point(|&line| line < self.offset);
                let start = before.checked_sub(1).unwrap_or(len - 1);
                (start + len - ((n - 1) % len)) % len
            }
        }
    }

    // -- misc ----------------------------------------------------------------

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    /// `r` / `^L` / `^R` repaint.
    pub fn repaint(&mut self) {
        self.force_redraw = true;
    }

    /// Set the case-sensitivity mode for searches.
    pub fn set_case_mode(&mut self, mode: CaseMode) {
        self.case_mode = mode;
    }

    /// Set the search-match highlight mode (`-g`/`-G`).
    pub fn set_highlight(&mut self, mode: HighlightMode) {
        self.highlight = mode;
    }

    /// `ESC-u`: toggle search highlight on/off. If the current mode is
    /// `None`, restore `All`; otherwise go to `None` (matching `less`).
    pub fn toggle_highlight(&mut self) {
        self.highlight = match self.highlight {
            HighlightMode::None => HighlightMode::All,
            _ => HighlightMode::None,
        };
    }

    /// `ESC-U`: clear the saved search pattern and turn highlighting off.
    pub fn clear_search(&mut self) {
        self.search = None;
        self.highlight = HighlightMode::None;
        self.clear_status_message();
    }

    // -- digit-prefix count --------------------------------------------------

    /// Append a digit (0-9) to the pending command count. Saturates at
    /// `usize::MAX` to prevent overflow.
    pub fn push_digit(&mut self, d: u8) {
        if d > 9 {
            return;
        }
        let cur = self.pending_count.unwrap_or(0);
        let new = cur.saturating_mul(10).saturating_add(d as usize);
        self.pending_count = Some(new);
    }

    /// Take the pending count, clearing it. Returns the count even if it is
    /// `0` (so `0G` can mean "go to top"); callers that treat `0` as
    /// "no count" should `.filter(|&n| n > 0)` themselves.
    pub fn take_count(&mut self) -> Option<usize> {
        self.pending_count.take()
    }

    /// Drop the pending count without using it (e.g. the user typed digits
    /// but pressed a non-counted key like `?`).
    pub fn clear_count(&mut self) {
        self.pending_count = None;
    }

    // -- folding -------------------------------------------------------------

    /// Line index (into `doc.lines`) where the section of heading `idx` ends
    /// (exclusive). This is the line before the next heading of the same or
    /// higher level, or the end of the document.
    fn section_end(&self, idx: usize) -> usize {
        let level = self.doc.headings[idx].level;
        for (_i, h) in self.doc.headings.iter().enumerate().skip(idx + 1) {
            if h.level <= level {
                return h.line;
            }
        }
        self.doc.lines.len()
    }

    /// True if `line` is hidden by a folded heading.
    fn is_hidden(&self, line: usize) -> bool {
        for &idx in &self.folded {
            let start = self.doc.headings[idx].line;
            let end = self.section_end(idx);
            if line > start && line < end {
                return true;
            }
        }
        false
    }

    /// Rebuild `visible_indices` from `doc.lines` minus folded sections.
    pub fn rebuild_visible_indices(&mut self) {
        self.visible_indices = (0..self.doc.lines.len())
            .filter(|&i| !self.is_hidden(i))
            .collect();
        self.offset = self.offset.min(self.max_offset());
    }

    /// Toggle the fold on the heading closest to the current view position.
    pub fn toggle_fold(&mut self) {
        if self.doc.headings.is_empty() {
            return;
        }
        let idx = self.closest_heading_index();
        let line = self.doc.headings[idx].line;
        let end = self.section_end(idx);
        // Only toggle if there's something to fold (at least one line after
        // the heading within the section).
        if end <= line + 1 {
            return;
        }
        if self.folded.contains(&idx) {
            self.folded.remove(&idx);
        } else {
            self.folded.insert(idx);
        }
        self.rebuild_visible_indices();
    }

    /// Jump to a doc-line index, unfolding any section that contains it.
    /// Converts the doc-line to a visible-row position and sets `offset`.
    pub fn jump_to_doc_line(&mut self, doc_line: usize) {
        self.dismiss_status_on_movement();
        // Unfold any section containing this line.
        let mut to_unfold: Vec<usize> = Vec::new();
        for &idx in &self.folded {
            let start = self.doc.headings[idx].line;
            let end = self.section_end(idx);
            if doc_line > start && doc_line < end {
                to_unfold.push(idx);
            }
        }
        let unfolded = !to_unfold.is_empty();
        for idx in to_unfold {
            self.folded.remove(&idx);
        }
        if unfolded {
            self.rebuild_visible_indices();
        }
        // Find the visible position for this doc line.
        self.offset = self
            .visible_indices
            .iter()
            .position(|&i| i >= doc_line)
            .unwrap_or(self.max_offset())
            .min(self.max_offset());
    }

    /// Returns the heading index whose line is at `doc_line`, if any.
    pub fn heading_at_doc_line(&self, doc_line: usize) -> Option<usize> {
        self.doc.headings.iter().position(|h| h.line == doc_line)
    }

    /// True if heading `idx` is currently folded.
    pub fn is_folded(&self, idx: usize) -> bool {
        self.folded.contains(&idx)
    }

    /// True if heading `idx` has foldable content (section body > 1 line).
    pub fn is_foldable(&self, idx: usize) -> bool {
        let line = self.doc.headings[idx].line;
        self.section_end(idx) > line + 1
    }

    // -- outline / heading navigation ---------------------------------------

    pub fn toggle_outline(&mut self) {
        self.show_outline = !self.show_outline;
        if self.show_outline {
            // Start selection at the heading closest to the current offset.
            self.outline_selection = self.closest_heading_index();
        }
    }

    /// Index of the heading whose line is closest to (and <=) the current
    /// offset. Returns 0 when there are no earlier headings.
    fn closest_heading_index(&self) -> usize {
        let headings = &self.doc.headings;
        if headings.is_empty() {
            return 0;
        }
        let current_doc_line = self.visible_indices.get(self.offset).copied().unwrap_or(0);
        let mut best = 0;
        for (i, h) in headings.iter().enumerate() {
            if h.line <= current_doc_line {
                best = i;
            } else {
                break;
            }
        }
        best
    }

    pub fn outline_next(&mut self) {
        if self.doc.headings.is_empty() {
            return;
        }
        self.outline_selection = (self.outline_selection + 1).min(self.doc.headings.len() - 1);
    }

    pub fn outline_prev(&mut self) {
        self.outline_selection = self.outline_selection.saturating_sub(1);
    }

    /// Jump to the selected heading and close the outline overlay.
    pub fn outline_jump(&mut self) {
        if let Some(h) = self.doc.headings.get(self.outline_selection) {
            let line = h.line;
            let text = h.text.clone();
            self.jump_to_doc_line(line);
            self.set_status_message(text);
        }
        self.show_outline = false;
    }

    /// Jump to the next heading after the current offset (wraps around).
    pub fn next_heading(&mut self) {
        let headings = &self.doc.headings;
        if headings.is_empty() {
            return;
        }
        let current_doc_line = self.visible_indices.get(self.offset).copied().unwrap_or(0);
        let next = headings.iter().position(|h| h.line > current_doc_line);
        let idx = next.unwrap_or(0); // wrap to first
        let line = headings[idx].line;
        let text = headings[idx].text.clone();
        self.jump_to_doc_line(line);
        self.set_status_message(text);
    }

    /// Jump to the previous heading before the current offset (wraps around).
    pub fn prev_heading(&mut self) {
        let headings = &self.doc.headings;
        if headings.is_empty() {
            return;
        }
        let current_doc_line = self.visible_indices.get(self.offset).copied().unwrap_or(0);
        let prev = headings.iter().rposition(|h| h.line < current_doc_line);
        let idx = match prev {
            Some(i) => i,
            None => headings.len() - 1, // wrap to last
        };
        let line = headings[idx].line;
        let text = headings[idx].text.clone();
        self.jump_to_doc_line(line);
        self.set_status_message(text);
    }
}

fn line_width(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

/// Gutter width for `n` lines: `digits(n) + 1` (for the trailing space).
fn gutter_width(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    n.to_string().len() + 1
}

/// Render a document, narrowing the wrap width by the gutter when line
/// numbers are enabled. Uses a two-pass approach: first render at full
/// width to count lines, then re-render at the narrowed width so the
/// gutter doesn't eat into the content.
fn mermaid_failure_message(n: usize) -> String {
    if n == 1 {
        "1 mermaid diagram failed to render (shown as source)".to_owned()
    } else {
        format!("{n} mermaid diagrams failed to render (shown as source)")
    }
}

fn render_doc(
    input: &Input,
    wrap_width: u16,
    line_numbers: bool,
    render_options: RenderOptions,
) -> Document {
    if !line_numbers {
        return Document::new_with_options(input, wrap_width, render_options);
    }
    let probe = Document::new_with_options(input, wrap_width, render_options);
    let g = gutter_width(probe.line_count());
    if g == 0 {
        return probe;
    }
    let narrowed = wrap_width.saturating_sub(g as u16).max(1);
    if narrowed == wrap_width {
        return probe;
    }
    let doc = Document::new_with_options(input, narrowed, render_options);
    // Rare: line count crossed a digit boundary after narrowing.
    let g2 = gutter_width(doc.line_count());
    if g2 == g {
        return doc;
    }
    let narrowed2 = wrap_width.saturating_sub(g2 as u16).max(1);
    Document::new_with_options(input, narrowed2, render_options)
}

fn content_width_for_doc(wrap_width: u16, line_numbers: bool, doc: &Document) -> usize {
    if line_numbers {
        wrap_width
            .saturating_sub(gutter_width(doc.line_count()) as u16)
            .max(1) as usize
    } else {
        wrap_width.max(1) as usize
    }
}

fn clip_line(line: &Line<'static>, start: usize, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::raw("");
    }

    let end = start + width;
    let mut col = 0;
    let mut spans = Vec::new();

    for span in &line.spans {
        let mut content = String::new();
        for ch in span.content.chars() {
            let ch_width = ch.width().unwrap_or(0);
            let ch_end = col + ch_width;

            if ch_width == 0 {
                if col > start && col <= end && !content.is_empty() {
                    content.push(ch);
                }
                continue;
            }

            if ch_end <= start {
                col = ch_end;
                continue;
            }
            if col >= end || ch_end > end {
                col = ch_end;
                break;
            }

            content.push(ch);
            col = ch_end;
        }

        if !content.is_empty() {
            spans.push(Span::styled(content, span.style));
        }

        if col >= end {
            break;
        }
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ResolvedMode;
    use ratatui::style::{Color, Style};

    fn make_state(text: &str, total_height: u16, width: u16) -> PagerState {
        let input = Input {
            text: text.to_owned(),
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        };
        PagerState::new(input, total_height, width, false)
    }

    fn doc_with_n_lines(n: usize) -> PagerState {
        let text: Vec<String> = (0..n).map(|i| format!("line {i}")).collect();
        make_state(&text.join("\n"), 10, 80)
    }

    fn plain(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn text_input(text: &str) -> Input {
        Input {
            text: text.to_owned(),
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        }
    }

    fn markdown_input(text: &str) -> Input {
        Input {
            text: text.to_owned(),
            render_mode: ResolvedMode::Markdown,
            source_path: None,
        }
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
    fn scroll_right_and_left_are_clamped() {
        let mut s = make_state("short", 10, 20);
        s.doc.lines = vec![Line::raw("0123456789abcdef")];
        s.width = 5;

        assert_eq!(s.max_h_offset(), 11);
        assert!(!s.take_force_redraw());
        s.scroll_right(8);
        assert_eq!(s.h_offset, 8);
        assert!(s.take_force_redraw());
        s.scroll_right(8);
        assert_eq!(s.h_offset, 11);
        assert!(s.take_force_redraw());
        s.scroll_right(8);
        assert_eq!(s.h_offset, 11);
        assert!(!s.take_force_redraw());
        s.scroll_left(4);
        assert_eq!(s.h_offset, 7);
        assert!(s.take_force_redraw());
        s.scroll_left(100);
        assert_eq!(s.h_offset, 0);
        assert!(s.take_force_redraw());
    }

    #[test]
    fn visible_lines_are_clipped_by_horizontal_offset() {
        let mut s = make_state("short", 10, 20);
        s.doc.lines = vec![Line::raw("0123456789abcdef")];
        s.width = 5;
        s.h_offset = 4;

        let lines = s.visible_lines_panned();
        assert_eq!(plain(&lines[0]), "45678");
    }

    #[test]
    fn viewport_overlay_replaces_top_visible_lines() {
        let mut s = doc_with_n_lines(5);
        s.set_viewport_overlay(vec![Line::raw("enhanced 0"), Line::raw("enhanced 1")]);

        let lines = s.visible_lines_panned();
        assert_eq!(plain(&lines[0]), "enhanced 0");
        assert_eq!(plain(&lines[1]), "enhanced 1");
    }

    #[test]
    fn viewport_overlay_only_applies_at_top() {
        let mut s = doc_with_n_lines(20);
        s.set_viewport_overlay(vec![Line::raw("enhanced 0")]);
        s.scroll_down(1);

        let lines = s.visible_lines_panned();
        assert_eq!(plain(&lines[0]), "line 1");
    }

    #[test]
    fn replace_doc_clears_viewport_overlay() {
        let mut s = doc_with_n_lines(5);
        s.set_viewport_overlay(vec![Line::raw("enhanced 0")]);
        let doc = Document {
            lines: vec![Line::raw("replacement")],
            headings: Vec::new(),
            source_path: None,
            mermaid_failures: 0,
        };
        s.replace_doc(doc, RenderOptions::default());

        assert!(s.viewport_overlay.is_none());
        assert_eq!(plain(&s.visible_lines_panned()[0]), "replacement");
    }

    #[test]
    fn replace_doc_with_mermaid_failures_sets_red_error_status() {
        let mut s = doc_with_n_lines(5);
        let doc = Document {
            lines: vec![Line::raw("replacement")],
            headings: Vec::new(),
            source_path: None,
            mermaid_failures: 2,
        };
        s.replace_doc(doc, RenderOptions::default());
        assert!(s.status_is_error());
        assert!(s.status.contains("2 mermaid diagrams failed"));
    }

    #[test]
    fn replace_doc_without_mermaid_failures_sets_no_error() {
        let mut s = doc_with_n_lines(5);
        let doc = Document {
            lines: vec![Line::raw("replacement")],
            headings: Vec::new(),
            source_path: None,
            mermaid_failures: 0,
        };
        s.replace_doc(doc, RenderOptions::default());
        assert!(!s.status_is_error());
        assert!(s.status.is_empty());
    }

    #[test]
    fn replace_doc_with_no_failures_clears_stale_error_status() {
        let mut s = doc_with_n_lines(5);
        s.set_error_message("previous doc had failures");
        assert!(s.status_is_error());

        let doc = Document {
            lines: vec![Line::raw("clean replacement")],
            headings: Vec::new(),
            source_path: None,
            mermaid_failures: 0,
        };
        s.replace_doc(doc, RenderOptions::default());
        assert!(!s.status_is_error(), "stale error flag must be cleared");
        assert!(s.status.is_empty(), "stale error text must be cleared");
    }

    #[test]
    fn error_status_is_dismissed_on_movement() {
        let mut s = doc_with_n_lines(50);
        s.set_error_message("boom");
        assert!(s.status_is_error());
        s.scroll_down(1);
        assert!(!s.status_is_error());
        assert!(s.status.is_empty());
    }

    #[test]
    fn normal_status_is_not_flagged_as_error() {
        let mut s = doc_with_n_lines(5);
        s.set_status_message("hint");
        assert!(!s.status_is_error());
    }

    #[test]
    fn replace_doc_clears_fold_state_for_new_headings() {
        let mut s = fold_state("# A\n\nbody\n\n# B\n\nother");
        s.toggle_fold();
        assert!(!s.folded.is_empty());

        let doc = Document::new(&text_input("replacement"), 80);
        s.replace_doc(doc, RenderOptions::default());

        assert!(s.folded.is_empty());
        assert_eq!(s.visible_indices, vec![0]);
    }

    #[test]
    fn replace_doc_clamps_outline_selection() {
        let mut s = fold_state("# A\n\nbody\n\n# B\n\nother");
        s.outline_selection = 1;
        let doc = Document::new(&markdown_input("# Only\n\nbody"), 80);

        s.replace_doc(doc, RenderOptions::default());

        assert_eq!(s.outline_selection, 0);
    }

    #[test]
    fn resize_clears_viewport_overlay() {
        let mut s = doc_with_n_lines(5);
        s.set_viewport_overlay(vec![Line::raw("enhanced 0")]);
        s.resize(10, 80);

        assert!(s.viewport_overlay.is_none());
        assert_eq!(plain(&s.visible_lines_panned()[0]), "line 0");
    }

    #[test]
    fn clipping_preserves_span_style() {
        let line = Line::from(vec![
            Span::raw("abc"),
            Span::styled("def", Style::default().fg(Color::Red)),
        ]);

        let clipped = clip_line(&line, 3, 2);
        assert_eq!(plain(&clipped), "de");
        assert_eq!(clipped.spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn table_hint_stays_gray_after_clipping() {
        let md = "| short | averyveryveryverylongcell |\n| --- | --- |\n| x | y |";
        let s = PagerState::new(markdown_input(md), 10, 30, false);
        let hint = s
            .visible_lines_panned()
            .into_iter()
            .find(|line| plain(line).contains("Table truncated"))
            .expect("expected table truncation hint");

        assert_eq!(hint.spans[0].style.fg, Some(Color::Gray));
    }

    #[test]
    fn resize_clamps_horizontal_offset() {
        let mut s = make_state("short", 10, 20);
        s.doc.lines = vec![Line::raw("0123456789")];
        s.width = 5;
        s.h_offset = 5;

        s.resize(10, 20);
        assert_eq!(s.h_offset, 0);
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
    fn table_mode_toggle_requests_full_redraw() {
        let mut s = md_state("| a | b |\n| --- | --- |\n| x | y |", 24, 20);
        assert!(!s.take_force_redraw());
        s.toggle_table_mode();
        assert!(s.take_force_redraw());
        assert!(!s.take_force_redraw());
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
    fn backward_search_repeats_in_reverse_direction() {
        let mut s = make_state("match\nother\nmatch\nother\nmatch", 2, 80);
        s.goto_bottom();
        s.start_backward_search();
        for c in "match".chars() {
            s.search_append(c);
        }
        s.finalize_search();

        assert_eq!(s.offset, 2);
        s.next_match();
        assert_eq!(s.offset, 0);
        s.prev_match();
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
    fn status_message_expires_after_ttl() {
        let mut s = make_state("hello", 10, 80);
        s.set_status_message("tables: expand");
        assert_eq!(s.status, "tables: expand");
        assert!(!s.tick_status(STATUS_MESSAGE_TTL_MS - 1));
        assert_eq!(s.status, "tables: expand");
        assert!(s.tick_status(1));
        assert!(s.status.is_empty());
    }

    #[test]
    fn scrolling_clears_status_message() {
        let mut s = make_state(&"a\n".repeat(50), 10, 80);
        s.set_status_message("tables: expand");
        s.scroll_down(1);
        assert!(s.status.is_empty());
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
            Mode::Search { ref query, .. } => assert_eq!(query, "x"),
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

    // -- outline / heading navigation tests --------------------------------

    fn md_state(md: &str, total_height: u16, width: u16) -> PagerState {
        let input = Input {
            text: md.to_owned(),
            render_mode: ResolvedMode::Markdown,
            source_path: None,
        };
        PagerState::new(input, total_height, width, false)
    }

    #[test]
    fn outline_captures_headings_from_markdown() {
        let s = md_state("# A\n\n## B\n\n### C\n", 24, 80);
        assert_eq!(s.doc.headings.len(), 3);
        assert_eq!(s.doc.headings[0].level, 1);
        assert_eq!(s.doc.headings[0].text, "A");
        assert_eq!(s.doc.headings[1].level, 2);
        assert_eq!(s.doc.headings[1].text, "B");
    }

    #[test]
    fn toggle_outline_sets_selection_to_closest_heading() {
        let mut s = md_state("# A\n\ntext\n\n## B\n\ntext\n\n## C\n", 2, 80);
        // Scroll to heading B.
        s.offset = 5;
        s.toggle_outline();
        assert!(s.show_outline);
        assert_eq!(s.outline_selection, 1); // heading B
    }

    #[test]
    fn outline_next_and_prev_move_selection() {
        let mut s = md_state("# A\n\n## B\n\n## C\n", 24, 80);
        s.toggle_outline();
        assert_eq!(s.outline_selection, 0);
        s.outline_next();
        assert_eq!(s.outline_selection, 1);
        s.outline_next();
        assert_eq!(s.outline_selection, 2);
        s.outline_next();
        assert_eq!(s.outline_selection, 2); // clamped at last
        s.outline_prev();
        assert_eq!(s.outline_selection, 1);
    }

    #[test]
    fn outline_jump_sets_offset_and_closes() {
        let mut s = md_state("# A\n\n## B\n\n## C\n", 2, 80);
        s.toggle_outline();
        s.outline_next(); // select B
        s.outline_jump();
        assert!(!s.show_outline);
        assert_eq!(s.offset, s.doc.headings[1].line);
        assert_eq!(s.status, "B");
    }

    #[test]
    fn next_heading_jumps_forward() {
        let mut s = md_state("# A\n\ntext\n\n## B\n\ntext\n\n## C\n", 2, 80);
        assert_eq!(s.offset, 0);
        s.next_heading();
        assert_eq!(s.offset, s.doc.headings[1].line);
        assert_eq!(s.status, "B");
        s.next_heading();
        assert_eq!(s.offset, s.doc.headings[2].line);
        assert_eq!(s.status, "C");
    }

    #[test]
    fn next_heading_wraps_to_first() {
        let mut s = md_state("# A\n\n## B\n", 2, 80);
        s.offset = s.doc.headings[1].line;
        s.next_heading(); // no heading after B -> wraps to A
        assert_eq!(s.offset, s.doc.headings[0].line);
        assert_eq!(s.status, "A");
    }

    #[test]
    fn prev_heading_jumps_backward() {
        let mut s = md_state("# A\n\ntext\n\n## B\n\ntext\n\n## C\n", 2, 80);
        s.offset = s.doc.headings[2].line;
        s.prev_heading();
        assert_eq!(s.offset, s.doc.headings[1].line);
        assert_eq!(s.status, "B");
    }

    #[test]
    fn prev_heading_wraps_to_last() {
        let mut s = md_state("# A\n\n## B\n", 2, 80);
        s.offset = 0;
        s.prev_heading(); // no heading before offset 0 -> wraps to B
        assert_eq!(s.offset, s.doc.headings[1].line);
        assert_eq!(s.status, "B");
    }

    #[test]
    fn heading_nav_noop_for_plain_text() {
        let mut s = make_state("no headings here", 24, 80);
        s.next_heading();
        assert_eq!(s.offset, 0);
        s.prev_heading();
        assert_eq!(s.offset, 0);
        s.toggle_outline();
        // No headings — selection stays at 0
        assert_eq!(s.outline_selection, 0);
    }

    // -- line numbers tests -------------------------------------------------

    #[test]
    fn gutter_width_zero_when_disabled() {
        let s = make_state("a\nb\nc", 24, 80);
        assert_eq!(s.gutter_width(), 0);
    }

    #[test]
    fn gutter_width_matches_line_count_digits() {
        let input = Input {
            text: "a\n".repeat(120).to_owned(),
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        };
        let s = PagerState::new(input, 24, 80, true);
        // 120 lines → 3 digits + 1 space = 4
        assert_eq!(s.gutter_width(), 4);
    }

    #[test]
    fn line_numbers_narrow_wrap_width() {
        let text = "aaaaaaaaaaaa".to_owned();
        let input = Input {
            text,
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        };
        let without = PagerState::new(input.clone(), 24, 10, false);
        let with = PagerState::new(input, 24, 10, true);
        // Without line numbers: wrap to 9 (10 - 1).
        // 12 chars / 9 = 2 lines.
        assert_eq!(without.line_count(), 2);
        // With line numbers: gutter = 2 (1 digit + space), wrap to 9 - 2 = 7.
        // 12 chars / 7 = 2 lines (7 + 5).
        assert!(with.line_count() >= 2);
        assert!(with.width < without.width);
    }

    // -- folding tests -------------------------------------------------------

    fn fold_state(md: &str) -> PagerState {
        let input = Input {
            text: md.to_owned(),
            render_mode: ResolvedMode::Markdown,
            source_path: None,
        };
        PagerState::new(input, 2, 80, false)
    }

    #[test]
    fn toggle_fold_hides_section_body() {
        let mut s = fold_state("# A\n\nbody line 1\n\nbody line 2\n\n# B\n\nother");
        let full = s.line_count();
        // Fold heading A (index 0).
        s.toggle_fold();
        assert!(s.line_count() < full, "folding should reduce visible lines");
        // Heading A's line should still be visible.
        assert!(s.visible_indices.contains(&0));
    }

    #[test]
    fn toggle_fold_then_unfold_restores() {
        let mut s = fold_state("# A\n\nbody line 1\n\nbody line 2\n\n# B");
        let full = s.line_count();
        s.toggle_fold();
        assert!(s.line_count() < full);
        s.toggle_fold();
        assert_eq!(s.line_count(), full);
    }

    #[test]
    fn folded_heading_line_still_visible() {
        let mut s = fold_state("# A\n\nbody\n\n## B\n\nbody2");
        // Fold A (index 0) — A's body (including B) should be hidden,
        // but A's own heading line should remain visible.
        s.toggle_fold();
        assert!(s.visible_indices.contains(&0));
        // B's heading line (index 1) should NOT be visible (it's inside A's fold).
        let b_line = s.doc.headings[1].line;
        assert!(!s.visible_indices.contains(&b_line));
    }

    #[test]
    fn jump_to_doc_line_unfolds_section() {
        let mut s = fold_state("# A\n\nbody line\n\n# B\n\nother");
        s.toggle_fold(); // fold A
        assert!(s.folded.contains(&0));
        // Jump to the body line of A (doc line 2).
        s.jump_to_doc_line(2);
        assert!(!s.folded.contains(&0), "should have unfolded A");
    }

    #[test]
    fn toggle_fold_noop_for_heading_without_body() {
        let mut s = fold_state("# A\n\n# B");
        let full = s.line_count();
        // A has only a blank line before B — nothing meaningful to fold.
        // (section_end(0) = headings[1].line, which is heading_line + 2 at most)
        // Toggle fold should either do nothing or only hide the blank line.
        s.toggle_fold();
        // Even if it folds, unfolding should restore.
        s.toggle_fold();
        assert_eq!(s.line_count(), full);
    }

    #[test]
    fn nested_folds_work() {
        let mut s = fold_state("# A\n\nbody\n\n## B\n\nbody2\n\n# C");
        // Fold B (index 1).
        s.offset = s
            .visible_indices
            .iter()
            .position(|&i| i == s.doc.headings[1].line)
            .unwrap();
        s.toggle_fold();
        assert!(s.folded.contains(&1));
        let folded_b = s.line_count();
        // Now fold A (index 0) — A's fold should hide everything including B.
        s.offset = 0;
        s.toggle_fold();
        assert!(s.folded.contains(&0));
        assert!(s.folded.contains(&1));
        assert!(s.line_count() < folded_b);
        // Unfold A — B should still be folded.
        s.toggle_fold();
        assert!(!s.folded.contains(&0));
        assert!(s.folded.contains(&1));
    }

    // -- repaint / case mode / highlight mode --------------------------------

    #[test]
    fn repaint_is_a_noop() {
        let mut s = make_state("hello\nworld", 24, 80);
        // Just verify it doesn't panic or change navigation state.
        s.repaint();
        assert_eq!(s.offset, 0);
        assert!(!s.quit);
        assert!(s.take_force_redraw());
    }

    #[test]
    fn set_case_mode_does_not_already_research() {
        // Changing case mode by itself shouldn't re-run an existing search.
        let mut s = make_state("Foo\nfoo", 24, 80);
        s.start_search();
        for c in "Foo".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert_eq!(s.search.as_ref().unwrap().matches.len(), 1);

        s.set_case_mode(CaseMode::Insensitive);
        // Same in-memory matches — set_case_mode is for the next search.
        assert_eq!(s.search.as_ref().unwrap().matches.len(), 1);

        // A new search DOES respect the new mode.
        s.cancel_search();
        s.start_search();
        for c in "foo".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert_eq!(s.search.as_ref().unwrap().matches.len(), 2);
    }

    #[test]
    fn case_mode_smart_falls_back_to_sensitive_for_uppercase_query() {
        let mut s = make_state("Foo\nfoo", 24, 80);
        s.set_case_mode(CaseMode::Smart);
        s.start_search();
        for c in "Foo".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        // Smart mode: uppercase in pattern => sensitive => only "Foo" matches.
        assert_eq!(s.search.as_ref().unwrap().matches.len(), 1);
    }

    #[test]
    fn toggle_highlight_flips_between_all_and_none() {
        let mut s = make_state("hello", 24, 80);
        assert_eq!(s.highlight, HighlightMode::All);
        s.toggle_highlight();
        assert_eq!(s.highlight, HighlightMode::None);
        s.toggle_highlight();
        assert_eq!(s.highlight, HighlightMode::All);
    }

    #[test]
    fn toggle_highlight_from_last_goes_to_none() {
        let mut s = make_state("hello", 24, 80);
        s.set_highlight(HighlightMode::Last);
        s.toggle_highlight();
        assert_eq!(s.highlight, HighlightMode::None);
    }

    #[test]
    fn clear_search_empties_search_and_disables_highlight() {
        let mut s = make_state("hello\nworld", 24, 80);
        s.start_search();
        for c in "hello".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert!(s.search.is_some());

        s.set_highlight(HighlightMode::Last);
        s.clear_search();
        assert!(s.search.is_none());
        assert_eq!(s.highlight, HighlightMode::None);
    }

    // -- digit-prefix count --------------------------------------------------

    #[test]
    fn push_digit_accumulates() {
        let mut s = make_state("hello", 24, 80);
        s.push_digit(1);
        s.push_digit(2);
        s.push_digit(3);
        assert_eq!(s.pending_count, Some(123));
    }

    #[test]
    fn push_digit_zero_starts_count() {
        // `0` is a real count start in less (e.g. `0G` = top).
        let mut s = make_state("hello", 24, 80);
        s.push_digit(0);
        assert_eq!(s.pending_count, Some(0));
    }

    #[test]
    fn push_digit_saturates_at_usize_max() {
        let mut s = make_state("hello", 24, 80);
        s.pending_count = Some(usize::MAX);
        s.push_digit(9);
        assert_eq!(s.pending_count, Some(usize::MAX));
    }

    #[test]
    fn take_count_returns_some_and_clears() {
        let mut s = make_state("hello", 24, 80);
        s.push_digit(5);
        assert_eq!(s.take_count(), Some(5));
        assert_eq!(s.pending_count, None);
    }

    #[test]
    fn take_count_returns_zero_for_nth_line_zero() {
        // `take_count` does NOT filter out zero — `0G` should be able to
        // reach `goto_line(0)` and jump to the top. Callers that treat
        // `0` as "no count" must filter themselves.
        let mut s = make_state("hello", 24, 80);
        s.push_digit(0);
        assert_eq!(s.take_count(), Some(0));
        assert_eq!(s.pending_count, None);
    }

    #[test]
    fn goto_line_zero_via_take_count_jumps_to_top() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.push_digit(0);
        let n = s.take_count().unwrap_or(0);
        s.goto_line(n);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn take_count_on_none_returns_none() {
        let mut s = make_state("hello", 24, 80);
        assert_eq!(s.take_count(), None);
    }

    #[test]
    fn clear_count_resets_pending() {
        let mut s = make_state("hello", 24, 80);
        s.push_digit(7);
        s.clear_count();
        assert_eq!(s.pending_count, None);
    }

    #[test]
    fn page_down_n_scrolls_n_pages() {
        let mut s = make_state(&"a\n".repeat(200), 24, 80);
        s.page_down_n(3);
        // height = 23 (24-1); page step is height - 1 = 22; 3 pages = 66.
        assert_eq!(s.offset, 66);
    }

    #[test]
    fn page_up_n_scrolls_back() {
        let mut s = make_state(&"a\n".repeat(200), 24, 80);
        s.offset = 100;
        s.page_up_n(2);
        // 2 pages of 22 = 44; 100 - 44 = 56.
        assert_eq!(s.offset, 56);
    }

    #[test]
    fn half_page_down_n_clamps_at_max() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.half_page_down_n(100);
        assert_eq!(s.offset, s.max_offset());
    }

    #[test]
    fn goto_line_zero_means_top() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.offset = 30;
        s.goto_line(0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn goto_line_is_1_based() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.goto_line(5);
        assert_eq!(s.offset, 4);
    }

    #[test]
    fn goto_line_clamps_to_last_visible() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.goto_line(9999);
        // jump_to_doc_line sets offset to the visible position of the line,
        // clamped to max_offset (= 50 - 23 = 27 for a 50-line, 23-tall view).
        assert_eq!(s.offset, 27);
    }

    #[test]
    fn goto_percent_zero_is_top() {
        let mut s = make_state(&"a\n".repeat(100), 24, 80);
        s.offset = 50;
        s.goto_percent(0);
        assert_eq!(s.offset, 0);
    }

    #[test]
    fn goto_percent_100_is_last() {
        let mut s = make_state(&"a\n".repeat(100), 24, 80);
        s.goto_percent(100);
        // 99 = last index, then jump_to_doc_line clamps to max_offset.
        assert!(s.offset <= s.max_offset());
    }

    #[test]
    fn goto_percent_50_lands_in_middle() {
        let mut s = make_state(&"a\n".repeat(100), 24, 80);
        s.goto_percent(50);
        // (100 - 1) * 0.5 = 49.5 → 49.
        assert_eq!(s.offset, 49);
    }

    #[test]
    fn finalize_search_uses_pending_count_as_nth_match() {
        // 5 matches; `3 /foo` should land on the 3rd.
        let mut s = make_state("foo\nbar\nfoo\nbar\nfoo\nbar\nfoo\nbar\nfoo", 24, 80);
        s.push_digit(3);
        s.start_search();
        for c in "foo".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        let search = s.search.as_ref().unwrap();
        assert_eq!(search.current, 2);
        assert_eq!(s.status, "3/5 matches");
    }

    #[test]
    fn finalize_search_without_count_defaults_to_first_match() {
        let mut s = make_state("foo\nbar\nfoo", 24, 80);
        s.start_search();
        for c in "foo".chars() {
            s.search_append(c);
        }
        s.finalize_search();
        assert_eq!(s.search.as_ref().unwrap().current, 0);
    }

    #[test]
    fn quit_at_eof_first_attempt_exits_on_scroll_past_eof() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.quit_at_eof = QuitAtEof::FirstAttempt;
        s.goto_bottom();
        assert!(!s.quit);
        s.scroll_down(1);
        assert!(s.quit);
    }

    #[test]
    fn quit_at_eof_second_attempt_requires_two_eof_scrolls() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.quit_at_eof = QuitAtEof::SecondAttempt;
        s.goto_bottom();
        s.scroll_down(1);
        assert!(!s.quit);
        s.scroll_down(1);
        assert!(s.quit);
    }

    #[test]
    fn quit_at_eof_resets_after_scrolling_away_from_eof() {
        let mut s = make_state(&"a\n".repeat(50), 24, 80);
        s.quit_at_eof = QuitAtEof::SecondAttempt;
        s.goto_bottom();
        s.scroll_down(1);
        s.scroll_up(1);
        s.scroll_down(1);
        assert!(!s.quit);
    }
}
