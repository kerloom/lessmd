//! lessmd entry point: terminal setup, event loop, and drawing.
//!
//! All pager logic is in the `lessmd` library crate (pure, unit-testable).
//! This file is the only place that performs terminal I/O via crossterm/
//! ratatui. `ratatui::run` handles raw mode, the alternate screen, and a
//! panic hook that restores the terminal before panicking.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
use lessmd::search::SearchDirection;
use lessmd::source::{self, ResolvedMode};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(50);

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

#[derive(Debug, Clone)]
struct AppOptions {
    line_numbers: bool,
    render_options: RenderOptions,
    quit_if_one_screen: bool,
    quit_on_intr: bool,
    case_mode: lessmd::search::CaseMode,
    highlight: lessmd::pager::HighlightMode,
    initial_command: Option<cli::InitialCommand>,
    quiet: bool,
    quit_at_eof: lessmd::pager::QuitAtEof,
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
    let options = AppOptions {
        line_numbers: args.line_numbers,
        render_options: RenderOptions {
            syntax: args.syntax,
            mermaid: args.mermaid,
            table_mode: args.table_mode,
        },
        quit_if_one_screen: args.quit_if_one_screen,
        quit_on_intr: args.quit_on_intr,
        case_mode: args.case_mode,
        highlight: args.highlight,
        initial_command: args.initial_command,
        quiet: args.quiet,
        quit_at_eof: args.quit_at_eof,
    };
    let input = match source::read(args.path.as_deref(), args.mode) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lessmd: {e}");
            std::process::exit(1);
        }
    };
    ratatui::run(|terminal| run_app(terminal, input, options))
}

fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    input: source::Input,
    mut options: AppOptions,
) -> std::io::Result<()> {
    let size = terminal.size()?;
    let prefix_source_lines = prefix_source_lines_for_height(size.height);
    let use_prefix =
        options.initial_command.is_none() && should_use_prefix_input(&input, prefix_source_lines);
    let first_input = if use_prefix {
        prefix_input_for_viewport(&input, prefix_source_lines)
    } else {
        input.clone()
    };
    let initial_options = initial_render_options(&input, options.render_options);
    let mut state = PagerState::new_with_options(
        first_input,
        size.height,
        size.width,
        options.line_numbers,
        initial_options,
    );
    // Keep the full input for resize/search after first paint. The initial doc
    // may be a prefix, but the state should know about the real source.
    state.input = input.clone();
    state.set_case_mode(options.case_mode);
    state.set_highlight(options.highlight);
    state.quiet = options.quiet;
    state.quit_at_eof = options.quit_at_eof;
    if let Some(command) = &options.initial_command {
        apply_initial_command(&mut state, command);
    }
    let (enhanced_tx, enhanced_rx) = mpsc::channel::<EnhancedMsg>();
    let mut render_generation = 0;
    if use_prefix || enhancements_differ(initial_options, options.render_options) {
        render_generation += 1;
        spawn_background_render(
            enhanced_tx.clone(),
            BackgroundRenderJob {
                input: input.clone(),
                height: size.height,
                width: size.width,
                line_numbers: options.line_numbers,
                current_options: initial_options,
                requested_options: options.render_options,
                prefix_source_lines,
                generation: render_generation,
            },
        );
    }
    let mut prev_offset = state.offset;
    let mut needs_draw = true;
    let mut pending_resize: Option<(u16, u16)> = None;
    let mut last_resize_at: Option<Instant> = None;
    let mut last_status_tick = Instant::now();
    loop {
        let status_delta_ms = last_status_tick
            .elapsed()
            .as_millis()
            .min(u128::from(u32::MAX)) as u32;
        last_status_tick = Instant::now();
        if state.tick_status(status_delta_ms) {
            needs_draw = true;
        }
        while let Ok(msg) = enhanced_rx.try_recv() {
            match msg {
                EnhancedMsg::Viewport { generation, lines } => {
                    if generation != render_generation {
                        continue;
                    }
                    state.set_viewport_overlay(lines);
                    terminal.clear()?;
                    needs_draw = true;
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
                    terminal.clear()?;
                    needs_draw = true;
                }
            }
        }

        if let (Some((w, h)), Some(last)) = (pending_resize, last_resize_at)
            && last.elapsed() >= RESIZE_DEBOUNCE
        {
            pending_resize = None;
            last_resize_at = None;
            render_generation += 1;
            state.resize(h, w);
            terminal.clear()?;
            needs_draw = true;
            if enhancements_differ(state.render_options, options.render_options) {
                spawn_background_render(
                    enhanced_tx.clone(),
                    BackgroundRenderJob {
                        input: state.input.clone(),
                        height: h,
                        width: w,
                        line_numbers: options.line_numbers,
                        current_options: state.render_options,
                        requested_options: options.render_options,
                        prefix_source_lines: prefix_source_lines_for_height(h),
                        generation: render_generation,
                    },
                );
            }
        }

        if needs_draw && pending_resize.is_none() {
            // Force a full redraw on multi-line jumps (Ctrl-D/U, PgUp/Dn, g, G)
            // to bypass ratatui's diff optimizer, which can leave stale content.
            if state.jumped(prev_offset) || state.take_force_redraw() {
                terminal.clear()?;
            }
            prev_offset = state.offset;
            terminal.draw(|frame| draw(frame, &mut state))?;
            needs_draw = false;
            // `-F`: exit immediately if the rendered document fits in the viewport
            // (status bar takes 1 line). Mirrors `less --quit-if-one-screen`.
            if options.quit_if_one_screen && state.doc.lines.len() <= state.height {
                return Ok(());
            }
        }

        let poll_timeout = last_resize_at
            .map(|last| resize_poll_timeout(last.elapsed()))
            .unwrap_or(EVENT_POLL_INTERVAL);
        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if options.quit_on_intr && is_interrupt_key(key) {
                        return Ok(());
                    }
                    input::handle_key(&mut state, key);
                    options.render_options.table_mode = state.render_options.table_mode;
                    needs_draw = true;
                    if state.quit {
                        return Ok(());
                    }
                }
                Event::Resize(w, h) => {
                    pending_resize = Some((w, h));
                    last_resize_at = Some(Instant::now());
                }
                _ => {}
            }
        }
    }
}

fn enhancements_differ(current: RenderOptions, requested: RenderOptions) -> bool {
    current.syntax != requested.syntax || current.mermaid != requested.mermaid
}

fn is_interrupt_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn apply_initial_command(state: &mut PagerState, command: &cli::InitialCommand) {
    match command {
        cli::InitialCommand::Bottom => state.goto_bottom(),
        cli::InitialCommand::Line(line) => state.goto_line(*line),
        cli::InitialCommand::Search { query, direction } => {
            state.apply_search(query.clone(), *direction);
        }
    }
}

fn resize_poll_timeout(elapsed: Duration) -> Duration {
    RESIZE_DEBOUNCE
        .saturating_sub(elapsed)
        .min(EVENT_POLL_INTERVAL)
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
            ..requested
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
    if let Some(search) = &state.search
        && state.highlight != lessmd::pager::HighlightMode::None
    {
        let query = &search.query;
        let current_doc_line = search.matches.get(search.current).copied();
        for (i, line) in lines.iter_mut().enumerate() {
            let doc_line = state.visible_indices.get(state.offset + i).copied();
            if let Some(dl) = doc_line {
                let is_current = current_doc_line == Some(dl);
                let should_highlight = match state.highlight {
                    lessmd::pager::HighlightMode::All => search.contains_line(dl),
                    lessmd::pager::HighlightMode::Last => is_current,
                    lessmd::pager::HighlightMode::None => false,
                };
                if should_highlight {
                    let current_range = if is_current {
                        lessmd::search::match_byte_range_with_case(line, query, state.case_mode)
                    } else {
                        None
                    };
                    *line = lessmd::search::highlight_line_with_case(
                        line,
                        query,
                        current_range,
                        state.case_mode,
                    );
                }
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
        Mode::Search { query, direction } => {
            let prompt = match direction {
                SearchDirection::Forward => '/',
                SearchDirection::Backward => '?',
            };
            Text::from(format!("{prompt}{}", sanitize_terminal_text(query)))
        }
        Mode::Normal => {
            // Digit-prefix count being built: show ":<n>" until consumed.
            if let Some(n) = state.pending_count {
                return Text::from(format!(":{n}"));
            }
            if !state.status.is_empty() {
                Text::from(sanitize_terminal_text(&state.status))
            } else {
                let name = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map(|n| sanitize_terminal_text(&n.to_string_lossy()))
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
                    Span::styled("  Press H for help", Style::default().fg(Color::Gray)),
                ]))
            }
        }
    }
}

fn sanitize_terminal_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
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
        .map(|h| heading_indent(h.level).len() + 2 + h.text.len())
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
        let icon = heading_icon(h.level);
        let style = if i == sel {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else {
            Style::default()
        };
        let level_color = heading_color(h.level);
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(icon, Style::default().fg(level_color)),
            Span::raw(" "),
        ];
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

fn heading_icon(level: u8) -> &'static str {
    match level {
        1 => "\u{2460}",
        2 => "\u{2461}",
        3 => "\u{2462}",
        4 => "\u{2463}",
        5 => "\u{2464}",
        _ => "\u{2465}",
    }
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
            table_mode: lessmd::render::TableMode::Expand,
        };
        assert_eq!(
            initial_render_options(&input(ResolvedMode::Markdown), requested),
            RenderOptions {
                syntax: false,
                mermaid: false,
                table_mode: lessmd::render::TableMode::Expand,
            }
        );
    }

    #[test]
    fn text_initial_render_keeps_requested_options() {
        let requested = RenderOptions {
            syntax: true,
            mermaid: true,
            table_mode: lessmd::render::TableMode::Truncate,
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
            table_mode: lessmd::render::TableMode::Truncate,
        };
        assert_eq!(
            initial_render_options(&input(ResolvedMode::Markdown), requested),
            requested
        );
    }

    #[test]
    fn enhancement_diff_ignores_table_mode() {
        let current = RenderOptions {
            syntax: true,
            mermaid: true,
            table_mode: lessmd::render::TableMode::Truncate,
        };
        let requested = RenderOptions {
            table_mode: lessmd::render::TableMode::Expand,
            ..current
        };
        assert!(!enhancements_differ(current, requested));
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

    #[test]
    fn sanitize_terminal_text_strips_control_sequences() {
        assert_eq!(
            sanitize_terminal_text("safe\x1b]52;c;bad\x07name"),
            "safe]52;c;badname"
        );
        assert_eq!(sanitize_terminal_text("line\nbreak"), "linebreak");
    }

    #[test]
    fn resize_poll_timeout_waits_until_debounce_deadline() {
        assert_eq!(
            resize_poll_timeout(Duration::from_millis(0)),
            EVENT_POLL_INTERVAL
        );
        assert_eq!(
            resize_poll_timeout(Duration::from_millis(40)),
            Duration::from_millis(10)
        );
        assert_eq!(
            resize_poll_timeout(Duration::from_millis(50)),
            Duration::ZERO
        );
    }

    #[test]
    fn interrupt_key_is_ctrl_c() {
        assert!(is_interrupt_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(!is_interrupt_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
        )));
        assert!(!is_interrupt_key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL,
        )));
    }
}
