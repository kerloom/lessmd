//! Substring search over rendered lines (the rendered text, per Q4).

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Case-sensitivity mode for `/` and `?` searches. Matches `less`'s `-i` and
/// `-I` flags: `Sensitive` is the default; `Smart` ignores case unless the
/// pattern itself contains uppercase; `Insensitive` always ignores case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaseMode {
    #[default]
    Sensitive,
    Smart,
    Insensitive,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    /// Indices into the document's `lines` vector.
    pub matches: Vec<usize>,
    /// Cursor into `matches` (0-based).
    pub current: usize,
}

/// Return true if `line` contains `query` according to `case_mode`.
fn line_contains(line: &Line, query: &str, case_mode: CaseMode) -> bool {
    let haystack = line_to_plain(line);
    match case_mode {
        CaseMode::Sensitive => haystack.contains(query),
        CaseMode::Insensitive => haystack.to_lowercase().contains(&query.to_lowercase()),
        CaseMode::Smart => {
            if query.chars().any(|c| c.is_uppercase()) {
                haystack.contains(query)
            } else {
                haystack.to_lowercase().contains(&query.to_lowercase())
            }
        }
    }
}

/// Return the indices of all lines containing `query`.
pub fn search_lines(lines: &[Line], query: &str, case_mode: CaseMode) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line_contains(line, query, case_mode))
        .map(|(i, _)| i)
        .collect()
}

/// Flatten a `Line`'s spans into a plain `String` for searching.
pub fn line_to_plain(line: &Line) -> String {
    let mut s = String::new();
    for span in &line.spans {
        s.push_str(&span.content);
    }
    s
}

/// Highlight all occurrences of `query` in `line` with the given style.
/// The `current_range` (if within this line) gets a brighter highlight.
pub fn highlight_line(
    line: &Line<'static>,
    query: &str,
    current_byte_range: Option<(usize, usize)>,
) -> Line<'static> {
    if query.is_empty() {
        return line.clone();
    }
    let plain = line_to_plain(line);
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    let mut start = 0;
    while let Some(pos) = plain[start..].find(query) {
        let abs = start + pos;
        let end = abs + query.len();
        let is_current = matches!(current_byte_range, Some((c, _)) if c == abs);
        ranges.push((abs, end, is_current));
        start = end;
    }
    if ranges.is_empty() {
        return line.clone();
    }

    // Light blue for non-current matches, orange for the current one. The
    // bold orange highlight is the conventional `less` color and stands out
    // against the soft periwinkle used for the other matches.
    let current_style = Style::default().fg(Color::Black).bg(Color::Yellow).bold();
    let other_style = Style::default().fg(Color::Black).bg(Color::LightBlue);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut byte_pos = 0usize;
    let mut range_idx = 0usize;

    for span in &line.spans {
        let content = span.content.as_ref();
        let span_start = byte_pos;
        let span_end = byte_pos + content.len();
        byte_pos = span_end;

        let mut cursor = 0usize;
        while cursor < content.len() {
            let abs_cursor = span_start + cursor;

            // Skip past ranges that end before this cursor.
            while range_idx < ranges.len() && ranges[range_idx].1 <= abs_cursor {
                range_idx += 1;
            }

            let (r_start, r_end, is_current) = match ranges.get(range_idx) {
                Some(&(rs, re, ic)) if rs < span_end && re > abs_cursor => (rs, re, ic),
                _ => {
                    // No range overlaps this cursor — emit the rest of the span.
                    let chunk = &content[cursor..];
                    if !chunk.is_empty() {
                        spans.push(Span::styled(chunk.to_owned(), span.style));
                    }
                    break;
                }
            };

            // Emit text before the match (within this span).
            let local_match_start = r_start.saturating_sub(span_start).max(cursor);
            if local_match_start > cursor {
                let before = &content[cursor..local_match_start];
                spans.push(Span::styled(before.to_owned(), span.style));
                cursor = local_match_start;
            }

            // Emit the matched portion (clamped to this span).
            let local_match_end = r_end.min(span_end) - span_start;
            let matched = &content[cursor..local_match_end];
            let base = if is_current {
                current_style
            } else {
                other_style
            };
            spans.push(Span::styled(
                matched.to_owned(),
                merge_styles(span.style, base),
            ));
            cursor = local_match_end;

            // Advance range_idx if this range is fully consumed.
            if span_start + cursor >= r_end {
                range_idx += 1;
            }
        }
    }

    Line::from(spans)
}

fn merge_styles(base: Style, highlight: Style) -> Style {
    let fg = highlight.fg.or(base.fg);
    let bg = highlight.bg.or(base.bg);
    let add_modifier = base.add_modifier | highlight.add_modifier;
    let sub_modifier = base.sub_modifier | highlight.sub_modifier;
    Style {
        fg,
        bg,
        add_modifier,
        sub_modifier,
        underline_color: base.underline_color,
    }
}

/// Find the byte offset of the first occurrence of `query` in a line's
/// plain text. Returns `None` if not found.
pub fn match_byte_offset(line: &Line, query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    line_to_plain(line).find(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    #[test]
    fn finds_matching_lines() {
        let lines = vec![Line::raw("alpha"), Line::raw("beta"), Line::raw("gamma")];
        // "a" appears in all three (alpha, bet**a**, gamm**a**)
        assert_eq!(
            search_lines(&lines, "a", CaseMode::Sensitive),
            vec![0, 1, 2]
        );
        assert_eq!(search_lines(&lines, "beta", CaseMode::Sensitive), vec![1]);
        assert_eq!(search_lines(&lines, "lph", CaseMode::Sensitive), vec![0]);
    }

    #[test]
    fn empty_query_matches_nothing() {
        let lines = vec![Line::raw("alpha")];
        assert!(search_lines(&lines, "", CaseMode::Sensitive).is_empty());
    }

    #[test]
    fn no_matches_returns_empty() {
        let lines = vec![Line::raw("alpha")];
        assert!(search_lines(&lines, "zzz", CaseMode::Sensitive).is_empty());
    }

    #[test]
    fn case_sensitive() {
        let lines = vec![Line::raw("Foo"), Line::raw("foo")];
        assert_eq!(search_lines(&lines, "Foo", CaseMode::Sensitive), vec![0]);
        assert_eq!(search_lines(&lines, "foo", CaseMode::Sensitive), vec![1]);
    }

    #[test]
    fn case_insensitive_ignores_case() {
        let lines = vec![Line::raw("Foo"), Line::raw("FOO"), Line::raw("bar")];
        assert_eq!(
            search_lines(&lines, "foo", CaseMode::Insensitive),
            vec![0, 1]
        );
    }

    #[test]
    fn smart_ignores_case_for_lowercase_pattern() {
        let lines = vec![Line::raw("Foo"), Line::raw("FOO"), Line::raw("bar")];
        assert_eq!(search_lines(&lines, "foo", CaseMode::Smart), vec![0, 1]);
    }

    #[test]
    fn smart_falls_back_to_sensitive_for_uppercase_pattern() {
        let lines = vec![Line::raw("Foo"), Line::raw("foo")];
        // Pattern has uppercase 'F' — case-sensitive match.
        assert_eq!(search_lines(&lines, "Foo", CaseMode::Smart), vec![0]);
    }

    // -- highlight_line tests ------------------------------------------------

    fn plain_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn highlight_preserves_text_content() {
        let line = Line::raw("hello world hello");
        let highlighted = highlight_line(&line, "hello", None);
        assert_eq!(plain_text(&highlighted), "hello world hello");
    }

    #[test]
    fn highlight_splits_at_match_boundaries() {
        let line = Line::raw("ab X cd");
        let highlighted = highlight_line(&line, "X", None);
        // Should have at least 3 spans: "ab ", "X", " cd"
        assert!(highlighted.spans.len() >= 3);
        assert_eq!(plain_text(&highlighted), "ab X cd");
    }

    #[test]
    fn highlight_applies_background_to_match() {
        let line = Line::raw("hello world");
        let highlighted = highlight_line(&line, "world", None);
        let match_span = highlighted
            .spans
            .iter()
            .find(|s| s.content == "world")
            .unwrap();
        assert!(
            match_span.style.bg.is_some(),
            "match should have a bg color"
        );
    }

    #[test]
    fn highlight_current_match_uses_yellow_background() {
        let line = Line::raw("foo bar foo");
        // First "foo" at byte 0
        let highlighted = highlight_line(&line, "foo", Some((0, 3)));
        let first_foo = highlighted
            .spans
            .iter()
            .find(|s| s.content == "foo" && s.style.bg == Some(Color::Yellow))
            .unwrap();
        // Second "foo" should have LightBlue bg
        let second_foo = highlighted
            .spans
            .iter()
            .filter(|s| s.content == "foo")
            .nth(1)
            .unwrap();
        assert_eq!(second_foo.style.bg, Some(Color::LightBlue));
        let _ = first_foo;
    }

    #[test]
    fn highlight_no_query_returns_original() {
        let line = Line::raw("hello");
        let result = highlight_line(&line, "", None);
        assert_eq!(plain_text(&result), "hello");
    }

    #[test]
    fn highlight_no_match_returns_original() {
        let line = Line::raw("hello");
        let result = highlight_line(&line, "zzz", None);
        assert_eq!(plain_text(&result), "hello");
    }

    #[test]
    fn highlight_multiple_matches_in_line() {
        let line = Line::raw("ab ab ab");
        let highlighted = highlight_line(&line, "ab", None);
        // All three "ab" spans should have a background
        let matched = highlighted
            .spans
            .iter()
            .filter(|s| s.content == "ab" && s.style.bg.is_some())
            .count();
        assert_eq!(matched, 3);
    }

    #[test]
    fn highlight_preserves_existing_span_styles() {
        use ratatui::style::Modifier;
        let line = Line::from(vec![
            Span::styled("bold ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("match"),
        ]);
        let highlighted = highlight_line(&line, "match", None);
        // The "match" span should still have its style + the highlight bg
        let match_span = highlighted
            .spans
            .iter()
            .find(|s| s.content == "match")
            .unwrap();
        assert!(match_span.style.bg.is_some());
        // The "bold " span should still be bold
        let bold_span = highlighted
            .spans
            .iter()
            .find(|s| s.content == "bold ")
            .unwrap();
        assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn match_byte_offset_finds_first_occurrence() {
        let line = Line::raw("hello world hello");
        assert_eq!(match_byte_offset(&line, "hello"), Some(0));
        assert_eq!(match_byte_offset(&line, "world"), Some(6));
        assert_eq!(match_byte_offset(&line, "zzz"), None);
        assert_eq!(match_byte_offset(&line, ""), None);
    }
}
