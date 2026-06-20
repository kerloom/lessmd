//! lessmd entry point: terminal setup, event loop, and drawing.
//!
//! All pager logic is in the `lessmd` library crate (pure, unit-testable).
//! This file is the only place that performs terminal I/O via crossterm/
//! ratatui. `ratatui::run` handles raw mode, the alternate screen, and a
//! panic hook that restores the terminal before panicking.

use std::sync::mpsc;
use std::time::Duration;

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
use lessmd::render::RenderOptions;
use lessmd::source::{self, ResolvedMode};

enum EnhancedMsg {
    Viewport {
        generation: u64,
        lines: Vec<Line<'static>>,
    },
    Full {
        generation: u64,
        doc: lessmd::document::Document,
        width: u16,
        options: RenderOptions,
    },
}

struct BackgroundRenderJob {
    input: source::Input,
    height: u16,
    width: u16,
    line_numbers: bool,
    current_options: RenderOptions,
    requested_options: RenderOptions,
    prefix_source_lines: usize,
    generation: u64,
}

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
    let render_options = RenderOptions {
        syntax: args.syntax,
        mermaid: args.mermaid,
    };
    let input = match source::read(args.path.as_deref(), args.mode) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lessmd: {e}");
            std::process::exit(1);
        }
    };
    ratatui::run(|terminal| run_app(terminal, input, line_numbers, render_options))
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    input: source::Input,
    line_numbers: bool,
    render_options: RenderOptions,
) -> std::io::Result<()> {
    let size = terminal.size()?;
    let prefix_source_lines = prefix_source_lines_for_height(size.height);
    let use_prefix = should_use_prefix_input(&input, prefix_source_lines);
    let first_input = if use_prefix {
        prefix_input_for_viewport(&input, prefix_source_lines)
    } else {
        input.clone()
    };
    let initial_options = initial_render_options(&input, render_options);
    let mut state = PagerState::new_with_options(
        first_input,
        size.height,
        size.width,
        line_numbers,
        initial_options,
    );
    // Keep the full input for resize/search after first paint. The initial doc
    // may be a prefix, but the state should know about the real source.
    state.input = input.clone();
    let (enhanced_tx, enhanced_rx) = mpsc::channel::<EnhancedMsg>();
    let mut render_generation = 0;
    if use_prefix || initial_options != render_options {
        render_generation += 1;
        spawn_background_render(
            enhanced_tx.clone(),
            BackgroundRenderJob {
                input: input.clone(),
                height: size.height,
                width: size.width,
                line_numbers,
                current_options: initial_options,
                requested_options: render_options,
                prefix_source_lines,
                generation: render_generation,
            },
        );
    }
    let mut prev_offset = state.offset;
    loop {
        while let Ok(msg) = enhanced_rx.try_recv() {
            match msg {
                EnhancedMsg::Viewport { generation, lines } => {
                    if generation != render_generation {
                        continue;
                    }
                    state.set_viewport_overlay(lines);
                    state.status = "enhanced viewport ready".to_owned();
                    terminal.clear()?;
                }
                EnhancedMsg::Full {
                    generation,
                    doc,
                    width,
                    options,
                } => {
                    if generation != render_generation {
                        continue;
                    }
                    state.width = width;
                    state.replace_doc(doc, options);
                    state.status = "enhanced render ready".to_owned();
                    terminal.clear()?;
                }
            }
        }
        // Force a full redraw on multi-line jumps (Ctrl-D/U, PgUp/Dn, g, G)
        // to bypass ratatui's diff optimizer, which can leave stale content.
        if state.jumped(prev_offset) {
            terminal.clear()?;
        }
        prev_offset = state.offset;
        terminal.draw(|frame| draw(frame, &mut state))?;
        if event::poll(Duration::from_millis(25))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    input::handle_key(&mut state, key);
                    if state.quit {
                        return Ok(());
                    }
                }
                Event::Resize(w, h) => {
                    render_generation += 1;
                    state.resize(h, w);
                    if state.render_options != render_options {
                        spawn_background_render(
                            enhanced_tx.clone(),
                            BackgroundRenderJob {
                                input: state.input.clone(),
                                height: h,
                                width: w,
                                line_numbers,
                                current_options: state.render_options,
                                requested_options: render_options,
                                prefix_source_lines: prefix_source_lines_for_height(h),
                                generation: render_generation,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn spawn_background_render(enhanced_tx: mpsc::Sender<EnhancedMsg>, job: BackgroundRenderJob) {
    std::thread::spawn(move || {
        if job.current_options != job.requested_options {
            let viewport_input = prefix_input_for_viewport(&job.input, job.prefix_source_lines);
            let viewport = PagerState::new_with_options(
                viewport_input,
                job.height,
                job.width,
                job.line_numbers,
                job.requested_options,
            );
            let overlay: Vec<Line<'static>> = viewport
                .doc
                .lines
                .into_iter()
                .take(viewport.height)
                .collect();
            let _ = enhanced_tx.send(EnhancedMsg::Viewport {
                generation: job.generation,
                lines: overlay,
            });
        }

        let enhanced = PagerState::new_with_options(
            job.input,
            job.height,
            job.width,
            job.line_numbers,
            job.requested_options,
        );
        let _ = enhanced_tx.send(EnhancedMsg::Full {
            generation: job.generation,
            doc: enhanced.doc,
            width: enhanced.width,
            options: job.requested_options,
        });
    });
}

fn initial_render_options(input: &source::Input, requested: RenderOptions) -> RenderOptions {
    if input.render_mode == ResolvedMode::Markdown && (requested.syntax || requested.mermaid) {
        RenderOptions {
            syntax: false,
            mermaid: false,
        }
    } else {
        requested
    }
}

fn prefix_source_lines_for_height(height: u16) -> usize {
    (height as usize).saturating_mul(20).max(1)
}

fn should_use_prefix_input(input: &source::Input, max_source_lines: usize) -> bool {
    input.text.lines().nth(max_source_lines).is_some()
}

fn prefix_input_for_viewport(input: &source::Input, max_source_lines: usize) -> source::Input {
    let mut text = input
        .text
        .lines()
        .take(max_source_lines.max(1))
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    source::Input {
        text,
        render_mode: input.render_mode.clone(),
        source_path: input.source_path.clone(),
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
    // Highlight search matches.
    if let Some(search) = &state.search {
        let query = &search.query;
        let current_doc_line = search.matches.get(search.current).copied();
        for (i, line) in lines.iter_mut().enumerate() {
            let doc_line = state.visible_indices.get(state.offset + i).copied();
            if let Some(dl) = doc_line
                && search.matches.contains(&dl)
            {
                let is_current = current_doc_line == Some(dl);
                let current_range = if is_current {
                    lessmd::search::match_byte_offset(line, query)
                        .map(|pos| (pos, pos + query.len()))
                } else {
                    None
                };
                *line = lessmd::search::highlight_line(line, query, current_range);
            }
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

    // Size the popup to fit the headings, capped at 90% of screen height.
    let max_h = frame.area().height.saturating_sub(2) as usize;
    let popup_h = headings.len().min(max_h).max(3) as u16;
    let area = centered_rect(70, popup_h, frame.area());

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

#[cfg(test)]
mod tests {
    use super::*;

    fn input(render_mode: ResolvedMode) -> source::Input {
        source::Input {
            text: String::new(),
            render_mode,
            source_path: None,
        }
    }

    #[test]
    fn markdown_initial_render_disables_enhancements() {
        let requested = RenderOptions {
            syntax: true,
            mermaid: true,
        };
        assert_eq!(
            initial_render_options(&input(ResolvedMode::Markdown), requested),
            RenderOptions {
                syntax: false,
                mermaid: false,
            }
        );
    }

    #[test]
    fn text_initial_render_keeps_requested_options() {
        let requested = RenderOptions {
            syntax: true,
            mermaid: true,
        };
        assert_eq!(
            initial_render_options(&input(ResolvedMode::Text { ansi: true }), requested),
            requested
        );
    }

    #[test]
    fn markdown_without_enhancements_does_not_need_second_phase() {
        let requested = RenderOptions {
            syntax: false,
            mermaid: false,
        };
        assert_eq!(
            initial_render_options(&input(ResolvedMode::Markdown), requested),
            requested
        );
    }

    #[test]
    fn prefix_input_keeps_only_requested_source_lines() {
        let input = source::Input {
            text: "a\nb\nc\nd".to_owned(),
            render_mode: ResolvedMode::Markdown,
            source_path: None,
        };

        let prefix = prefix_input_for_viewport(&input, 2);
        assert_eq!(prefix.text, "a\nb\n");
        assert_eq!(prefix.render_mode, ResolvedMode::Markdown);
    }

    #[test]
    fn should_use_prefix_when_input_exceeds_limit() {
        let input = source::Input {
            text: "a\nb\nc".to_owned(),
            render_mode: ResolvedMode::Text { ansi: true },
            source_path: None,
        };
        assert!(should_use_prefix_input(&input, 2));
        assert!(!should_use_prefix_input(&input, 3));
    }

    #[test]
    fn prefix_source_lines_scales_with_terminal_height() {
        assert_eq!(prefix_source_lines_for_height(0), 1);
        assert_eq!(prefix_source_lines_for_height(1), 20);
        assert_eq!(prefix_source_lines_for_height(24), 480);
    }

    #[test]
    fn prefix_input_always_keeps_at_least_one_line() {
        let input = source::Input {
            text: "a\nb".to_owned(),
            render_mode: ResolvedMode::Markdown,
            source_path: None,
        };

        let prefix = prefix_input_for_viewport(&input, 0);
        assert_eq!(prefix.text, "a\n");
    }
}
