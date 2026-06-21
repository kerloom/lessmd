//! Help overlay content (pure data; the popup itself is drawn in `main.rs`).

use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

pub fn help_text() -> Text<'static> {
    let title = Line::from(Span::styled(
        "lessmd keybindings",
        Style::default().fg(Color::Cyan).bold(),
    ));
    let mut lines: Vec<Line<'static>> = vec![title, Line::raw("")];

    let entries: [(&str, &str); 26] = [
        ("j / e / Down", "scroll down N lines (1 if no count)"),
        ("k / y / Up", "scroll up N lines (1 if no count)"),
        ("h / <-", "pan left N columns (8 if no count)"),
        ("l / ->", "pan right N columns (8 if no count)"),
        ("Space / f / PgDn", "scroll down N pages"),
        ("b / PgUp", "scroll up N pages"),
        ("Ctrl-D", "scroll down N half-pages"),
        ("Ctrl-U", "scroll up N half-pages"),
        ("g / Home", "go to line N (top if no count)"),
        ("G / End", "go to line N (bottom if no count)"),
        ("N p / N %", "go to N percent into the file"),
        ("t", "next heading"),
        ("T", "previous heading"),
        ("o", "toggle (o)utline (jump to heading)"),
        ("Tab", "toggle fold on heading"),
        ("w", "toggle table truncate/expand (w)idth"),
        ("/", "start search (preceded by N = Nth match)"),
        ("?", "start backward search"),
        ("n", "next match"),
        ("N", "previous match"),
        ("r / Ctrl-L", "repaint (no-op)"),
        ("Esc-u", "toggle search-match highlighting"),
        ("Esc-U", "clear saved search + highlighting"),
        ("Ctrl-C", "abort search"),
        ("H", "toggle this (H)elp"),
        ("q / Q / Esc", "quit"),
    ];

    for (key, desc) in entries {
        lines.push(Line::from(vec![
            Span::styled(format!("  {key:<20}"), Style::default().fg(Color::Yellow)),
            Span::raw(desc),
        ]));
    }

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &Text<'static>) -> String {
        text.lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn help_lists_help_key_and_horizontal_pan_keys() {
        let text = plain(&help_text());
        assert!(text.contains("H"));
        assert!(text.contains("toggle this (H)elp"));
        assert!(text.contains("start backward search"));
        assert!(text.contains("h / <-"));
        assert!(text.contains("l / ->"));
        assert!(text.contains("toggle (o)utline"));
        assert!(text.contains("next heading"));
        assert!(text.contains("Tab"));
        assert!(text.contains("toggle fold"));
        assert!(text.contains("toggle table truncate/expand (w)idth"));
    }
}
