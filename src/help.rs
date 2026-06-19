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

    let entries: [(&str, &str); 14] = [
        ("j / e / Down", "scroll down one line"),
        ("k / y / Up", "scroll up one line"),
        ("Space / f / PgDn", "scroll down one page"),
        ("b / PgUp", "scroll up one page"),
        ("Ctrl-D", "scroll down half a page"),
        ("Ctrl-U", "scroll up half a page"),
        ("g / Home", "go to top"),
        ("G / End", "go to bottom"),
        ("/", "start search"),
        ("n", "next match"),
        ("N", "previous match"),
        ("Ctrl-C", "abort search"),
        ("h / H", "toggle this help"),
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
