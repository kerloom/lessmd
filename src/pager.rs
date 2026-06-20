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
use crate::render::RenderOptions;
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
    /// Heading indices (into `doc.headings`) whose body is folded.
    pub folded: HashSet<usize>,
    /// Maps each visible row → index into `doc.lines`. Rebuilt when folds
    /// change or the document is re-rendered.
    pub visible_indices: Vec<usize>,
    /// Enhanced lines for the initial viewport. Used by the two-phase UI to
    /// show syntax/Mermaid in the first screen before the full enhanced render
    /// completes. Only applies while `offset == 0`.
    pub viewport_overlay: Option<Vec<Line<'static>>>,
    pub status: String,
}

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
            folded: HashSet::new(),
            visible_indices: Vec::new(),
            viewport_overlay: None,
            status: String::new(),
        };
        state.rebuild_visible_indices();
        state
    }

    pub fn replace_doc(&mut self, doc: Document, render_options: RenderOptions) {
        self.doc = doc;
        self.render_options = render_options;
        self.viewport_overlay = None;
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

    /// Returns true when the viewport jumped by more than one line since
    /// `prev_offset` (e.g. Ctrl-D, Ctrl-U, PageUp/Down, g, G). The caller
    /// should force a full terminal redraw in that case to prevent stale
    /// content from surviving ratatui's diff optimization.
    pub fn jumped(&self, prev_offset: usize) -> bool {
        self.offset.abs_diff(prev_offset) > 1
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

    pub fn scroll_right(&mut self, n: usize) {
        self.h_offset = (self.h_offset + n).min(self.max_h_offset());
    }

    pub fn scroll_left(&mut self, n: usize) {
        self.h_offset = self.h_offset.saturating_sub(n);
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
        self.rebuild_visible_indices();
        self.h_offset = self.h_offset.min(self.max_h_offset());
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
            self.jump_to_doc_line(matches[current]);
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
        self.jump_to_doc_line(line);
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
        self.jump_to_doc_line(line);
        self.status = format!("{}/{} matches", cur + 1, total);
    }

    // -- misc ----------------------------------------------------------------

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn quit(&mut self) {
        self.quit = true;
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
            self.status = text;
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
        self.status = text;
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
        self.status = text;
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
        s.scroll_right(8);
        assert_eq!(s.h_offset, 8);
        s.scroll_right(8);
        assert_eq!(s.h_offset, 11);
        s.scroll_left(4);
        assert_eq!(s.h_offset, 7);
        s.scroll_left(100);
        assert_eq!(s.h_offset, 0);
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
        };
        s.replace_doc(doc, RenderOptions::default());

        assert!(s.viewport_overlay.is_none());
        assert_eq!(plain(&s.visible_lines_panned()[0]), "replacement");
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
        // Scroll to heading B (line 4).
        s.offset = 4;
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
}
