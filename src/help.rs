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

    let entries: [(&str, &str); 23] = [
        ("j / e / v", "scroll down one line"),
        ("k / y / ^", "scroll up one line"),
        ("h / <-", "pan left"),
        ("l / ->", "pan right"),
        ("Space / f / PgDn", "scroll down one page"),
        ("b / PgUp", "scroll up one page"),
        ("Ctrl-D", "scroll down half a page"),
        ("Ctrl-U", "scroll up half a page"),
        ("g / Home", "go to top"),
        ("G / End", "go to bottom"),
        ("t", "next heading"),
        ("T", "previous heading"),
        ("o", "toggle outline (jump to heading)"),
        ("Tab", "toggle fold on heading"),
        ("/", "start search"),
        ("n", "next match"),
        ("N", "previous match"),
        ("r / Ctrl-L", "repaint (no-op)"),
        ("Esc-u", "toggle search-match highlighting"),
        ("Esc-U", "clear saved search + highlighting"),
        ("Ctrl-C", "abort search"),
        ("?", "toggle this help"),
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
    fn help_lists_question_mark_and_horizontal_pan_keys() {
        let text = plain(&help_text());
        assert!(text.contains("?"));
        assert!(text.contains("toggle this help"));
        assert!(text.contains("h / <-"));
        assert!(text.contains("l / ->"));
        assert!(text.contains("outline"));
        assert!(text.contains("next heading"));
        assert!(text.contains("Tab"));
        assert!(text.contains("toggle fold"));
    }
}
