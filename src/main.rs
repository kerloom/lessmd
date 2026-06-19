//! lessmd entry point: terminal setup, event loop, and drawing.
//!
//! All pager logic is in the `lessmd` library crate (pure, unit-testable).
//! This file is the only place that performs terminal I/O via crossterm/
//! ratatui. `ratatui::run` handles raw mode, the alternate screen, and a
//! panic hook that restores the terminal before panicking.

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use lessmd::cli;
use lessmd::help;
use lessmd::input;
use lessmd::pager::{Mode, PagerState};
use lessmd::source;

fn main() -> std::io::Result<()> {
    let args = match cli::parse(std::env::args()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("lessmd: {e}");
            std::process::exit(2);
        }
    };
    if args.show_help {
        print!("{}", cli::HELP_TEXT);
        return Ok(());
    }
    if args.show_version {
        println!("lessmd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let line_numbers = args.line_numbers;
    let input = match source::read(args.path.as_deref(), args.mode) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lessmd: {e}");
            std::process::exit(1);
        }
    };
    ratatui::run(|terminal| run_app(terminal, input, line_numbers))
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    input: source::Input,
    line_numbers: bool,
) -> std::io::Result<()> {
    let size = terminal.size()?;
    let mut state = PagerState::new(input, size.height, size.width, line_numbers);
    let mut prev_offset = state.offset;
    loop {
        // Force a full redraw on multi-line jumps (Ctrl-D/U, PgUp/Dn, g, G)
        // to bypass ratatui's diff optimizer, which can leave stale content.
        if state.jumped(prev_offset) {
            terminal.clear()?;
        }
        prev_offset = state.offset;
        terminal.draw(|frame| draw(frame, &mut state))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                input::handle_key(&mut state, key);
                if state.quit {
                    return Ok(());
                }
            }
            Event::Resize(w, h) => state.resize(h, w),
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, state: &mut PagerState) {
    let [main, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

    let mut lines = state.visible_lines_panned();
    // Add fold indicators on heading lines.
    for (i, line) in lines.iter_mut().enumerate() {
        if let Some(doc_line) = state.visible_indices.get(state.offset + i).copied()
            && let Some(hidx) = state.heading_at_doc_line(doc_line)
            && state.is_foldable(hidx)
        {
            let marker = if state.is_folded(hidx) {
                "▸ "
            } else {
                "▾ "
            };
            line.spans
                .insert(0, Span::styled(marker, Style::default().fg(Color::Gray)));
        }
    }
    if state.line_numbers {
        let gw = state.gutter_width();
        let digits = gw.saturating_sub(1);
        for (i, line) in lines.iter_mut().enumerate() {
            let doc_line = state.visible_indices.get(state.offset + i).copied();
            let num = doc_line.map(|d| d + 1).unwrap_or(0);
            line.spans.insert(
                0,
                Span::styled(
                    format!("{:>width$} ", num, width = digits),
                    Style::default().fg(Color::Gray),
                ),
            );
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), main);

    frame.render_widget(
        Paragraph::new(status_line(state)).style(Style::default().fg(Color::Yellow)),
        status_area,
    );

    if state.show_help {
        draw_help(frame);
    }
    if state.show_outline {
        draw_outline(frame, state);
    }
}

fn status_line(state: &PagerState) -> Text<'static> {
    match &state.mode {
        Mode::Search(q) => Text::from(format!("/{q}")),
        Mode::Normal => {
            if !state.status.is_empty() {
                Text::from(state.status.clone())
            } else {
                let name = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "stdin".to_owned());
                let mut status = format!("{}  {}%", name, percentage(state));
                if state.max_h_offset() > 0 {
                    status.push_str(&format!(
                        "  col {}/{}",
                        state.h_offset + 1,
                        state.max_line_width()
                    ));
                }
                Text::from(Line::from(vec![
                    Span::raw(status),
                    Span::styled("  ? help", Style::default().fg(Color::Gray)),
                ]))
            }
        }
    }
}

fn percentage(state: &PagerState) -> u16 {
    let total = state.line_count();
    if total == 0 {
        return 100;
    }
    let bottom = state.offset + state.height;
    ((bottom as u64 * 100 / total as u64) as u16).min(100)
}

fn draw_help(frame: &mut Frame) {
    let area = centered_rect(70, 80, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(help::help_text())
            .block(Block::default().borders(Borders::ALL).title(" help ")),
        area,
    );
}

fn draw_outline(frame: &mut Frame, state: &PagerState) {
    let headings = &state.doc.headings;
    if headings.is_empty() {
        let area = centered_rect(40, 20, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("No headings found.")
                .style(Style::default().fg(Color::Gray))
                .block(Block::default().borders(Borders::ALL).title(" outline ")),
            area,
        );
        return;
    }

    // Size the popup to fit the headings, capped at 80% of the screen.
    let max_h = frame.area().height.saturating_sub(4) as usize;
    let popup_h = headings.len().min(max_h).max(3) as u16;
    let area = centered_rect(65, popup_h, frame.area());

    // Compute the longest heading line to size the popup width.
    let max_text_len = headings
        .iter()
        .map(|h| heading_indent(h.level).len() + h.text.len())
        .max()
        .unwrap_or(0);
    let inner_w = (area.width as usize).saturating_sub(2);
    let _ = max_text_len.min(inner_w);

    // Build the heading lines with indentation and selection highlight.
    let sel = state.outline_selection;
    let inner_h = (area.height as usize).saturating_sub(2);
    let scroll = sel
        .saturating_sub(inner_h / 2)
        .min(headings.len().saturating_sub(inner_h).max(0));

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, h) in headings.iter().enumerate() {
        let indent = heading_indent(h.level);
        let style = if i == sel {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default()
        };
        let level_color = heading_color(h.level);
        let mut spans = vec![Span::raw(indent)];
        spans.push(Span::styled(h.text.clone(), style.fg(level_color)));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .scroll((scroll as u16, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" outline (Enter=jump, Esc=close) "),
            ),
        area,
    );
}

fn heading_indent(level: u8) -> String {
    "  ".repeat(level.saturating_sub(1) as usize)
}

fn heading_color(level: u8) -> Color {
    match level {
        1 => Color::Cyan,
        2 => Color::Blue,
        3 => Color::Green,
        4 => Color::Yellow,
        5 => Color::Magenta,
        _ => Color::Gray,
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [h] = Layout::horizontal([Constraint::Percentage(percent_x)]).areas(area);
    let [_, m, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(h);
    m
}
