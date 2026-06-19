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
    text::Text,
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
    let input = match source::read(args.path.as_deref(), args.mode) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lessmd: {e}");
            std::process::exit(1);
        }
    };
    ratatui::run(|terminal| run_app(terminal, input))
}

fn run_app(terminal: &mut ratatui::DefaultTerminal, input: source::Input) -> std::io::Result<()> {
    let size = terminal.size()?;
    let mut state = PagerState::new(input, size.height, size.width);
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

    frame.render_widget(
        Paragraph::new(Text::from(state.visible_lines_panned())),
        main,
    );

    frame.render_widget(
        Paragraph::new(status_line(state)).style(Style::default().fg(Color::Yellow)),
        status_area,
    );

    if state.show_help {
        draw_help(frame);
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
                Text::from(status)
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
