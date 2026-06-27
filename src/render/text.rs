//! Plain-text renderer: split into lines, optionally interpret ANSI SGR
//! escape sequences as styles, and hard-wrap to the viewport width.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::RenderOutput;

/// Render `text` into a flat list of terminal lines, each wrapped to `width`.
///
/// When `ansi` is true, CSI SGR sequences (`\x1b[...m`) are interpreted as
/// ratatui styles; other escape sequences (OSC, bare ESC) are stripped.
/// When `ansi` is false, all escape sequences are stripped.
pub fn render_text(text: &str, width: u16, ansi: bool) -> RenderOutput {
    let width = width.max(1) as usize;
    let lines: Vec<Line<'static>> = text
        .lines()
        .flat_map(|raw| {
            let line = if ansi {
                parse_ansi_line(raw)
            } else {
                strip_ansi_line(raw)
            };
            wrap_line(&line, width)
        })
        .collect();
    RenderOutput {
        lines,
        headings: Vec::new(),
        mermaid_failures: 0,
    }
}

// ---------------------------------------------------------------------------
// ANSI handling
// ---------------------------------------------------------------------------

/// If a CSI escape (`ESC [ params final`) starts at byte `i`, returns
/// `(index_after_final_byte, final_char, params)`.
fn parse_csi(s: &str, i: usize) -> Option<(usize, char, &str)> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if i + 1 >= len || bytes[i] != 0x1b || bytes[i + 1] != b'[' {
        return None;
    }
    let start = i + 2;
    let mut j = start;
    while j < len && (0x30..=0x3f).contains(&bytes[j]) {
        j += 1;
    }
    while j < len && (0x20..=0x2f).contains(&bytes[j]) {
        j += 1;
    }
    if j >= len {
        return None;
    }
    let final_byte = bytes[j] as char;
    Some((j + 1, final_byte, &s[start..j]))
}

/// Skip a non-CSI escape (OSC sequence, or a bare 2-byte `ESC x`).
/// Returns the index just past the escape.
fn skip_other_escape(s: &str, i: usize) -> usize {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if i + 1 >= len {
        return len;
    }
    if bytes[i + 1] == b']' {
        // OSC: terminated by BEL (0x07) or ST (ESC \).
        let mut j = i + 2;
        while j < len {
            if bytes[j] == 0x07 {
                return j + 1;
            }
            if bytes[j] == 0x1b && j + 1 < len && bytes[j + 1] == b'\\' {
                return j + 2;
            }
            j += 1;
        }
        return len;
    }
    // Bare ESC + one byte.
    i + 2
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

fn flush(buf: &mut String, spans: &mut Vec<Span<'static>>, style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(buf.clone(), style));
        buf.clear();
    }
}

/// Parse a single source line (no newlines) into a styled `Line`.
fn parse_ansi_line(s: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut buf = String::new();
    let mut i = 0;
    let len = s.len();

    while i < len {
        if let Some((next, final_byte, params)) = parse_csi(s, i) {
            if final_byte == 'm' {
                flush(&mut buf, &mut spans, style);
                style = apply_sgr(params, style);
            }
            i = next;
            continue;
        }
        if s.as_bytes()[i] == 0x1b {
            i = skip_other_escape(s, i);
            continue;
        }
        let ch_end = next_char_boundary(s, i);
        buf.push_str(&s[i..ch_end]);
        i = ch_end;
    }
    flush(&mut buf, &mut spans, style);

    if spans.is_empty() {
        Line::raw("")
    } else {
        Line::from(spans)
    }
}

/// Strip all escape sequences; return an unstyled `Line` of the literal text.
fn strip_ansi_line(s: &str) -> Line<'static> {
    let mut out = String::new();
    let mut i = 0;
    let len = s.len();

    while i < len {
        if let Some((next, _, _)) = parse_csi(s, i) {
            i = next;
            continue;
        }
        if s.as_bytes()[i] == 0x1b {
            i = skip_other_escape(s, i);
            continue;
        }
        let ch_end = next_char_boundary(s, i);
        out.push_str(&s[i..ch_end]);
        i = ch_end;
    }
    Line::raw(out)
}

/// Apply an SGR parameter string to `style`, returning the new style.
fn apply_sgr(params: &str, mut style: Style) -> Style {
    if params.is_empty() {
        return Style::default();
    }
    let nums: Vec<u16> = params
        .split(';')
        .filter_map(|p| p.parse::<u16>().ok())
        .collect();
    let mut k = 0;
    while k < nums.len() {
        let n = nums[k];
        match n {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(standard_color(n - 30)),
            38 => {
                if let Some((c, consumed)) = parse_extended_color(&nums[k + 1..]) {
                    style = style.fg(c);
                    k += consumed;
                }
            }
            39 => style = style.fg(Color::Reset),
            40..=47 => style = style.bg(standard_color(n - 40)),
            48 => {
                if let Some((c, consumed)) = parse_extended_color(&nums[k + 1..]) {
                    style = style.bg(c);
                    k += consumed;
                }
            }
            49 => style = style.bg(Color::Reset),
            90..=97 => style = style.fg(bright_color(n - 90)),
            100..=107 => style = style.bg(bright_color(n - 100)),
            _ => {}
        }
        k += 1;
    }
    style
}

fn standard_color(n: u16) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        _ => Color::Reset,
    }
}

fn bright_color(n: u16) -> Color {
    match n {
        0 => Color::DarkGray,
        1 => Color::LightRed,
        2 => Color::LightGreen,
        3 => Color::LightYellow,
        4 => Color::LightBlue,
        5 => Color::LightMagenta,
        6 => Color::LightCyan,
        7 => Color::White,
        _ => Color::Reset,
    }
}

/// Parse a 256-color (`5;n`) or truecolor (`2;r;g;b`) sequence tail.
/// Returns `(color, params_consumed)` (not counting the leading 38/48).
fn parse_extended_color(nums: &[u16]) -> Option<(Color, usize)> {
    let mode = *nums.first()?;
    match mode {
        5 => {
            let idx = nums.get(1).copied().unwrap_or(0) as u8;
            Some((Color::Indexed(idx), 2))
        }
        2 => {
            let r = nums.get(1).copied().unwrap_or(0) as u8;
            let g = nums.get(2).copied().unwrap_or(0) as u8;
            let b = nums.get(3).copied().unwrap_or(0) as u8;
            Some((Color::Rgb(r, g, b), 4))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Wrapping
// ---------------------------------------------------------------------------

/// Wrap a single (already styled) `Line` into multiple lines no wider than
/// `width` cells, splitting spans as needed.
pub(crate) fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line.clone()];
    }
    let mut result: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for span in &line.spans {
        let style = span.style;
        let mut remaining = span.content.as_ref();
        while !remaining.is_empty() {
            let avail = width.saturating_sub(current_width);
            if avail == 0 {
                result.push(Line::from(std::mem::take(&mut current)));
                current_width = 0;
                continue;
            }
            let (chunk, chunk_width, rest) = take_chunk(remaining, avail);
            current.push(Span::styled(chunk.to_owned(), style));
            current_width += chunk_width;
            remaining = rest;
        }
    }

    if !current.is_empty() || result.is_empty() {
        result.push(Line::from(current));
    }
    result
}

/// Take the longest run of `s` whose unicode width fits `avail` cells.
/// Always advances at least one character (so wide chars eventually flush).
///
/// Uses `UnicodeWidthStr::width` (not per-char sum) so emoji variation
/// sequences like `⚠️` are measured correctly.
fn take_chunk(s: &str, avail: usize) -> (&str, usize, &str) {
    let mut end = 0;
    for (i, ch) in s.char_indices() {
        let candidate_end = i + ch.len_utf8();
        let w = UnicodeWidthStr::width(&s[..candidate_end]);
        if w > avail {
            break;
        }
        end = candidate_end;
    }
    let chunk = &s[..end];
    let width = UnicodeWidthStr::width(chunk);
    if end == 0 {
        // No char fit (first char wider than `avail`, or a zero-width char):
        // force at least one char to make progress.
        if let Some((i, ch)) = s.char_indices().next() {
            end = i + ch.len_utf8();
            return (&s[..end], UnicodeWidthStr::width(&s[..end]), &s[end..]);
        }
    }
    (&s[..end], width, &s[end..])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        let mut s = String::new();
        for span in &line.spans {
            s.push_str(&span.content);
        }
        s
    }

    #[test]
    fn wraps_long_line_to_width() {
        let lines = render_text("aaaaaaaaaa", 4, false).lines;
        assert_eq!(lines.len(), 3);
        assert_eq!(plain(&lines[0]), "aaaa");
        assert_eq!(plain(&lines[1]), "aaaa");
        assert_eq!(plain(&lines[2]), "aa");
    }

    #[test]
    fn ansi_passthrough_preserves_color() {
        let lines = render_text("\x1b[31mred\x1b[0m", 80, true).lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(plain(&lines[0]), "red");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn ansi_passthrough_preserves_bold_and_underline() {
        let lines = render_text("\x1b[1mbold\x1b[22m \x1b[4mu\x1b[24m", 80, true).lines;
        assert_eq!(plain(&lines[0]), "bold u");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn ansi_stripped_in_plain_mode() {
        let lines = render_text("\x1b[31mred\x1b[0m", 80, false).lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(plain(&lines[0]), "red");
        assert_eq!(lines[0].spans[0].style.fg, None);
    }

    #[test]
    fn osc_sequences_stripped_in_ansi_mode() {
        // OSC 8 hyperlink wrapper — text inside must survive, escapes stripped.
        let lines = render_text("\x1b]8;;https://example.com\x07link\x1b]8;;\x07", 80, true).lines;
        assert_eq!(plain(&lines[0]), "link");
    }

    #[test]
    fn truecolor_parsed() {
        let lines = render_text("\x1b[38;2;10;20;30mx\x1b[0m", 80, true).lines;
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn empty_input_yields_no_lines() {
        let lines = render_text("", 80, false).lines;
        assert!(lines.is_empty());
    }

    #[test]
    fn multiple_lines_preserved() {
        let lines = render_text("a\nb\nc", 80, false).lines;
        assert_eq!(lines.len(), 3);
        assert_eq!(plain(&lines[0]), "a");
        assert_eq!(plain(&lines[2]), "c");
    }

    #[test]
    fn crlf_split_into_single_line() {
        let lines = render_text("a\r\nb", 80, false).lines;
        assert_eq!(lines.len(), 2);
        assert_eq!(plain(&lines[0]), "a");
        assert_eq!(plain(&lines[1]), "b");
    }

    #[test]
    fn zero_width_char_makes_progress() {
        // combining acute accent (U+0301) is zero-width; must not loop forever
        let lines = render_text("e\u{0301}", 1, false).lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(plain(&lines[0]), "e\u{0301}");
    }
}
