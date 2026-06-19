//! Substring search over rendered lines (the rendered text, per Q4).

use ratatui::text::Line;

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    /// Indices into the document's `lines` vector.
    pub matches: Vec<usize>,
    /// Cursor into `matches` (0-based).
    pub current: usize,
}

/// Return the indices of all lines containing `query` (case-sensitive).
pub fn search_lines(lines: &[Line], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line_to_plain(line).contains(query))
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    #[test]
    fn finds_matching_lines() {
        let lines = vec![Line::raw("alpha"), Line::raw("beta"), Line::raw("gamma")];
        // "a" appears in all three (alpha, bet**a**, gamm**a**)
        assert_eq!(search_lines(&lines, "a"), vec![0, 1, 2]);
        assert_eq!(search_lines(&lines, "beta"), vec![1]);
        assert_eq!(search_lines(&lines, "lph"), vec![0]);
    }

    #[test]
    fn empty_query_matches_nothing() {
        let lines = vec![Line::raw("alpha")];
        assert!(search_lines(&lines, "").is_empty());
    }

    #[test]
    fn no_matches_returns_empty() {
        let lines = vec![Line::raw("alpha")];
        assert!(search_lines(&lines, "zzz").is_empty());
    }

    #[test]
    fn case_sensitive() {
        let lines = vec![Line::raw("Foo"), Line::raw("foo")];
        assert_eq!(search_lines(&lines, "Foo"), vec![0]);
    }
}
