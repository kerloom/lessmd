//! Markdown renderer: pulldown-cmark events -> `Vec<Line<'static>>`.
//!
//! Iterates a pull parser, maintaining a small stack of inline style mods
//! and block-level context (list nesting, blockquote depth, code blocks,
//! tables). Emits pre-wrapped lines with prefixes (blockquote bars / list
//! bullets) already attached.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::RenderOutput;
use super::mermaid::{DefaultMermaidRenderer, MermaidRenderer};
use super::text::wrap_line;
use crate::document::Heading;

/// Render markdown `text` into a flat list of terminal lines wrapped to `width`.
pub fn render_markdown(text: &str, width: u16) -> RenderOutput {
    let renderer = DefaultMermaidRenderer;
    render_markdown_with_mermaid(text, width, &renderer)
}

/// Render markdown with an injected Mermaid renderer.
pub fn render_markdown_with_mermaid(
    text: &str,
    width: u16,
    mermaid: &dyn MermaidRenderer,
) -> RenderOutput {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;
    let parser = Parser::new_ext(text, opts);
    let mut r = MdRenderer::new(width.max(1) as usize, mermaid);
    for event in parser {
        r.handle(event);
    }
    r.finish();
    r.into_output()
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ListInfo {
    ordered: bool,
    counter: u64,
}

#[derive(Debug, Clone)]
enum StackEntry {
    Bold,
    Italic,
    Strike,
    Link(String),
    Image(String),
}

#[derive(Debug, Default)]
struct TableBuilder {
    rows: Vec<Vec<String>>,
    aligns: Vec<Alignment>,
    current_row: Vec<String>,
    header: Option<Vec<String>>,
}

struct MdRenderer<'a> {
    width: usize,
    mermaid: &'a dyn MermaidRenderer,
    out: Vec<Line<'static>>,
    pending: Vec<Span<'static>>,
    /// Base style for the current block (heading color, blockquote dim, ...).
    block_style: Style,
    /// Inline style stack (bold/italic/strike/link/image).
    stack: Vec<StackEntry>,
    list_stack: Vec<ListInfo>,
    quote_depth: usize,
    item_bullet: Option<String>,
    item_first: bool,
    first_prefix: String,
    cont_prefix: String,
    code_buf: Option<String>,
    code_lang: Option<String>,
    table: Option<TableBuilder>,
    cell_buf: String,
    in_cell: bool,
    /// Headings captured during rendering for the outline / jump-to-heading.
    headings: Vec<Heading>,
    /// `Some(level)` while inside a heading; `None` otherwise.
    pending_heading: Option<HeadingLevel>,
}

impl<'a> MdRenderer<'a> {
    fn new(width: usize, mermaid: &'a dyn MermaidRenderer) -> Self {
        Self {
            width,
            mermaid,
            out: Vec::new(),
            pending: Vec::new(),
            block_style: Style::default(),
            stack: Vec::new(),
            list_stack: Vec::new(),
            quote_depth: 0,
            item_bullet: None,
            item_first: false,
            first_prefix: String::new(),
            cont_prefix: String::new(),
            code_buf: None,
            code_lang: None,
            table: None,
            cell_buf: String::new(),
            in_cell: false,
            headings: Vec::new(),
            pending_heading: None,
        }
    }

    fn into_output(self) -> RenderOutput {
        RenderOutput {
            lines: self.out,
            headings: self.headings,
        }
    }

    // -- dispatch ------------------------------------------------------------

    fn handle(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => self.inline_code(&t),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.rule(),
            Event::TaskListMarker(checked) => self.task_marker(checked),
            Event::Html(t) | Event::InlineHtml(t) => self.html(&t),
            _ => {}
        }
    }

    // -- start tags ----------------------------------------------------------

    fn start(&mut self, tag: Tag) {
        // In tight lists pulldown-cmark emits no Paragraph events, so inline
        // content from the previous item/block would otherwise merge into the
        // next. Flush before starting any new block (but not for inline tags).
        if is_block_tag(&tag) {
            self.flush();
        }
        match tag {
            Tag::Paragraph => {
                self.block_style = self.context_style();
                self.pending.clear();
            }
            Tag::Heading { level, .. } => {
                self.block_style = heading_style(level);
                self.pending.clear();
                self.pending_heading = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.quote_depth += 1;
                self.recompute_prefix();
            }
            Tag::CodeBlock(kind) => {
                self.code_buf = Some(String::new());
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => Some(lang.into_string()),
                    CodeBlockKind::Indented => None,
                };
            }
            Tag::List(start) => {
                self.list_stack.push(ListInfo {
                    ordered: start.is_some(),
                    counter: start.unwrap_or(0),
                });
                self.recompute_prefix();
            }
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.stack.push(StackEntry::Italic),
            Tag::Strong => self.stack.push(StackEntry::Bold),
            Tag::Strikethrough => self.stack.push(StackEntry::Strike),
            Tag::Link { dest_url, .. } => self.stack.push(StackEntry::Link(dest_url.into_string())),
            Tag::Image { dest_url, .. } => {
                self.stack.push(StackEntry::Image(dest_url.into_string()))
            }
            Tag::Table(aligns) => {
                self.table = Some(TableBuilder {
                    rows: Vec::new(),
                    aligns,
                    current_row: Vec::new(),
                    header: None,
                });
            }
            Tag::TableHead => {}
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.current_row = Vec::new();
                }
            }
            Tag::TableCell => {
                self.in_cell = true;
                self.cell_buf.clear();
            }
            Tag::HtmlBlock => {
                self.pending.clear();
            }
            _ => {}
        }
    }

    // -- end tags ------------------------------------------------------------

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                self.flush();
                self.push_blank();
            }
            TagEnd::Heading(_) => {
                if let Some(level) = self.pending_heading.take() {
                    let text: String = self
                        .pending
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                        .trim()
                        .to_owned();
                    let line = self.out.len();
                    self.headings.push(Heading {
                        level: level as u8,
                        text,
                        line,
                    });
                }
                self.flush();
                self.push_blank();
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.recompute_prefix();
                self.push_blank();
            }
            TagEnd::CodeBlock => {
                self.flush_code_block();
                self.push_blank();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.recompute_prefix();
                self.push_blank();
            }
            TagEnd::Item => {
                self.flush();
                self.item_bullet = None;
                self.item_first = false;
                self.recompute_prefix();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.stack.pop();
            }
            TagEnd::Link => {
                if let Some(StackEntry::Link(url)) = self.stack.pop() {
                    if self.in_cell {
                        self.cell_buf.push_str(&format!(" ({url})"));
                    } else {
                        self.pending
                            .push(Span::styled(format!(" ({url})"), Style::default().dim()));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(StackEntry::Image(url)) = self.stack.pop() {
                    if self.in_cell {
                        self.cell_buf.push_str(&format!(" ({url})"));
                    } else {
                        self.pending
                            .push(Span::styled(format!(" ({url})"), Style::default().dim()));
                    }
                }
            }
            TagEnd::Table => {
                self.flush_table();
                self.push_blank();
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.header = Some(std::mem::take(&mut t.current_row));
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.rows.push(std::mem::take(&mut t.current_row));
                }
            }
            TagEnd::TableCell => {
                self.in_cell = false;
                if let Some(t) = self.table.as_mut() {
                    t.current_row.push(self.cell_buf.clone());
                }
            }
            TagEnd::HtmlBlock => {
                self.flush();
                self.push_blank();
            }
            _ => {}
        }
    }

    // -- inline content ------------------------------------------------------

    fn text(&mut self, t: &str) {
        if let Some(buf) = &mut self.code_buf {
            buf.push_str(t);
            return;
        }
        if self.in_cell {
            self.cell_buf.push_str(t);
            return;
        }
        let style = self.current_style();
        self.pending.push(Span::styled(t.to_owned(), style));
    }

    fn inline_code(&mut self, t: &str) {
        if self.in_cell {
            self.cell_buf.push_str(t);
            return;
        }
        let style = self.current_style().fg(Color::Yellow);
        self.pending.push(Span::styled(t.to_owned(), style));
    }

    fn html(&mut self, t: &str) {
        if let Some(buf) = &mut self.code_buf {
            buf.push_str(t);
            return;
        }
        if self.in_cell {
            self.cell_buf.push_str(t);
            return;
        }
        let style = self.current_style().dim();
        for (i, line) in t.split('\n').enumerate() {
            if i > 0 {
                self.flush();
            }
            if !line.is_empty() {
                self.pending.push(Span::styled(line.to_owned(), style));
            }
        }
    }

    fn soft_break(&mut self) {
        if let Some(last) = self.pending.last_mut() {
            last.content.to_mut().push(' ');
        } else {
            self.pending.push(Span::raw(" "));
        }
    }

    fn hard_break(&mut self) {
        self.flush();
    }

    fn rule(&mut self) {
        self.flush();
        let prefix = self.cont_prefix.clone();
        let rule_w = self.width.saturating_sub(width_of(&prefix));
        let mut spans = vec![Span::raw(prefix)];
        spans.push(Span::styled(
            "─".repeat(rule_w),
            Style::default().fg(Color::DarkGray),
        ));
        self.out.push(Line::from(spans));
        self.push_blank();
    }

    fn task_marker(&mut self, checked: bool) {
        self.item_bullet = Some(if checked { "☑ " } else { "☐ " }.to_owned());
        self.item_first = true;
        self.recompute_prefix();
    }

    // -- style / prefix helpers ----------------------------------------------

    fn context_style(&self) -> Style {
        if self.quote_depth > 0 {
            Style::default().dim()
        } else {
            Style::default()
        }
    }

    fn current_style(&self) -> Style {
        let mut s = self.block_style;
        for entry in &self.stack {
            match entry {
                StackEntry::Bold => s = s.add_modifier(Modifier::BOLD),
                StackEntry::Italic => s = s.add_modifier(Modifier::ITALIC),
                StackEntry::Strike => s = s.add_modifier(Modifier::CROSSED_OUT),
                StackEntry::Link(_) => s = s.add_modifier(Modifier::UNDERLINED).fg(Color::Blue),
                StackEntry::Image(_) => s = s.add_modifier(Modifier::ITALIC),
            }
        }
        s
    }

    fn start_item(&mut self) {
        let bullet = match self.list_stack.last_mut() {
            Some(l) if l.ordered => {
                let b = format!("{}. ", l.counter);
                l.counter += 1;
                b
            }
            _ => "• ".to_owned(),
        };
        self.item_bullet = Some(bullet);
        self.item_first = true;
        self.recompute_prefix();
    }

    fn recompute_prefix(&mut self) {
        let parent_indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
        let quote = "│ ".repeat(self.quote_depth);
        match &self.item_bullet {
            Some(b) => {
                self.first_prefix = format!("{quote}{parent_indent}{b}");
                self.cont_prefix = format!("{quote}{parent_indent}{}", " ".repeat(width_of(b)));
            }
            None => {
                let base = format!("{quote}{parent_indent}");
                self.first_prefix = base.clone();
                self.cont_prefix = base;
            }
        }
    }

    fn prefix_style(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }

    // -- flushing ------------------------------------------------------------

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let content = std::mem::take(&mut self.pending);
        let prefix = if self.item_first {
            self.first_prefix.clone()
        } else {
            self.cont_prefix.clone()
        };
        self.item_first = false;
        let prefix_w = width_of(&prefix);
        let avail = self.width.saturating_sub(prefix_w).max(1);
        let wrapped = wrap_line(&Line::from(content), avail);
        if wrapped.is_empty() {
            self.out
                .push(prefix_line(&prefix, self.quote_depth, self.prefix_style()));
            return;
        }
        for (i, wl) in wrapped.iter().enumerate() {
            let p = if i == 0 { &prefix } else { &self.cont_prefix };
            let mut spans = Vec::new();
            if !p.is_empty() {
                spans.push(prefix_span(p, self.quote_depth, self.prefix_style()));
            }
            spans.extend(wl.spans.iter().cloned());
            self.out.push(Line::from(spans));
        }
    }

    fn flush_code_block(&mut self) {
        let code = self.code_buf.take().unwrap_or_default();
        let lang = self.code_lang.take();
        if is_mermaid_lang(lang.as_deref()) {
            self.flush_mermaid_block(&code);
            return;
        }

        self.push_code_block(&code, lang.as_deref());
    }

    fn flush_mermaid_block(&mut self, code: &str) {
        match self.mermaid.render(code) {
            Ok(rendered) => self.push_rendered_mermaid(&rendered),
            Err(err) => {
                self.push_code_block(code, Some("mermaid"));
                self.push_mermaid_note(&err);
            }
        }
    }

    fn push_code_block(&mut self, code: &str, lang: Option<&str>) {
        let prefix = self.cont_prefix.clone();
        let prefix_w = width_of(&prefix);
        let avail = self.width.saturating_sub(prefix_w).max(1);
        let pfx_style = self.prefix_style();

        if let Some(l) = lang
            && !l.is_empty()
        {
            let mut spans = Vec::new();
            if !prefix.is_empty() {
                spans.push(prefix_span(&prefix, self.quote_depth, pfx_style));
            }
            spans.push(Span::styled(
                format!("┌─ {l}"),
                Style::default().fg(Color::Gray),
            ));
            self.out.push(Line::from(spans));
        }

        let highlighted: Option<Vec<Line<'static>>> = {
            #[cfg(feature = "syntax")]
            {
                lang.and_then(|l| {
                    let token = l.split_whitespace().next().unwrap_or("");
                    super::syntax::highlight_code(code, token)
                })
            }
            #[cfg(not(feature = "syntax"))]
            {
                None
            }
        };

        if let Some(hl_lines) = highlighted {
            for hl_line in &hl_lines {
                let wrapped = wrap_line(hl_line, avail);
                for wl in &wrapped {
                    let mut spans = Vec::new();
                    if !prefix.is_empty() {
                        spans.push(prefix_span(&prefix, self.quote_depth, pfx_style));
                    }
                    spans.extend(wl.spans.iter().cloned());
                    self.out.push(Line::from(spans));
                }
            }
        } else {
            let code_style = Style::default().fg(Color::Yellow);
            for line in code.lines() {
                if line.is_empty() {
                    self.out
                        .push(prefix_line(&prefix, self.quote_depth, pfx_style));
                    continue;
                }
                let wrapped = wrap_line(&Line::styled(line.to_owned(), code_style), avail);
                for wl in &wrapped {
                    let mut spans = Vec::new();
                    if !prefix.is_empty() {
                        spans.push(prefix_span(&prefix, self.quote_depth, pfx_style));
                    }
                    spans.extend(wl.spans.iter().cloned());
                    self.out.push(Line::from(spans));
                }
            }
        }
    }

    fn push_rendered_mermaid(&mut self, rendered: &str) {
        let prefix = self.cont_prefix.clone();
        let avail = self.width.saturating_sub(width_of(&prefix)).max(1);
        let pfx_style = self.prefix_style();
        let diagram_style = Style::default().fg(Color::Cyan);

        if rendered.lines().any(|line| width_of(line) > avail) {
            self.push_mermaid_pan_hint();
        }

        for line in rendered.lines() {
            let mut spans = Vec::new();
            if !prefix.is_empty() {
                spans.push(prefix_span(&prefix, self.quote_depth, pfx_style));
            }
            if line.is_empty() {
                self.out.push(Line::from(spans));
            } else {
                spans.push(Span::styled(line.to_owned(), diagram_style));
                self.out.push(Line::from(spans));
            }
        }
    }

    fn push_mermaid_pan_hint(&mut self) {
        let prefix = self.cont_prefix.clone();
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(prefix_span(&prefix, self.quote_depth, self.prefix_style()));
        }
        spans.push(Span::styled(
            "Use <-/-> or h/l to pan",
            Style::default().fg(Color::Gray),
        ));
        self.out.push(Line::from(spans));
    }

    fn push_mermaid_note(&mut self, err: &str) {
        let prefix = self.cont_prefix.clone();
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(prefix_span(&prefix, self.quote_depth, self.prefix_style()));
        }
        spans.push(Span::styled(
            format!("mermaid render failed: {err}"),
            Style::default().fg(Color::Gray),
        ));
        self.out.push(Line::from(spans));
    }

    fn flush_table(&mut self) {
        let Some(tbl) = self.table.take() else {
            return;
        };
        let lines = render_table_grid(tbl, self.width);
        self.out.extend(lines);
    }

    fn push_blank(&mut self) {
        if self.out.last().map(is_blank).unwrap_or(true) {
            return;
        }
        self.out.push(Line::raw(""));
    }

    fn finish(&mut self) {
        self.flush();
        while self.out.last().map(is_blank).unwrap_or(false) {
            self.out.pop();
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn width_of(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn is_mermaid_lang(lang: Option<&str>) -> bool {
    lang.and_then(|l| l.split_whitespace().next())
        .is_some_and(|l| l.eq_ignore_ascii_case("mermaid"))
}

fn is_block_tag(tag: &Tag) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
    )
}

fn is_blank(line: &Line) -> bool {
    line.spans.iter().all(|s| s.content.is_empty())
}

fn prefix_span(prefix: &str, quote_depth: usize, style: Style) -> Span<'static> {
    if quote_depth > 0 {
        Span::styled(prefix.to_owned(), style)
    } else {
        Span::raw(prefix.to_owned())
    }
}

fn prefix_line(prefix: &str, quote_depth: usize, style: Style) -> Line<'static> {
    Line::from(vec![prefix_span(prefix, quote_depth, style)])
}

fn heading_style(level: HeadingLevel) -> Style {
    let color = match level {
        HeadingLevel::H1 => Color::Cyan,
        HeadingLevel::H2 => Color::Blue,
        HeadingLevel::H3 => Color::Green,
        HeadingLevel::H4 => Color::Yellow,
        HeadingLevel::H5 => Color::Magenta,
        HeadingLevel::H6 => Color::Gray,
    };
    let mut s = Style::default().fg(color).bold();
    if level <= HeadingLevel::H2 {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

fn render_table_grid(tbl: TableBuilder, width: usize) -> Vec<Line<'static>> {
    let ncol = tbl.aligns.len().max(1);
    let mut all_rows: Vec<Vec<String>> = Vec::new();
    if let Some(h) = tbl.header {
        all_rows.push(h);
    }
    all_rows.extend(tbl.rows);

    // Natural column widths (max content width per column).
    let mut natural_w = vec![0usize; ncol];
    for row in &all_rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncol {
                natural_w[i] = natural_w[i].max(width_of(cell));
            }
        }
    }

    // Total table width = 1 (left border) + sum(col_w + 3) for each column
    // (2 padding spaces + 1 right border). Fit within viewport.
    let col_w = fit_columns(&natural_w, width);

    let border_style = Style::default().fg(Color::DarkGray);
    let header_style = Style::default().bold();
    let mut out = Vec::new();

    out.push(Line::styled(
        make_border('┌', '┬', '┐', &col_w),
        border_style,
    ));
    if let Some(header) = all_rows.first() {
        out.push(make_row(header, &tbl.aligns, &col_w, header_style));
        out.push(Line::styled(
            make_border('├', '┼', '┤', &col_w),
            border_style,
        ));
    }
    for row in all_rows.iter().skip(1) {
        out.push(make_row(row, &tbl.aligns, &col_w, Style::default()));
    }
    out.push(Line::styled(
        make_border('└', '┴', '┘', &col_w),
        border_style,
    ));
    out
}

/// Shrink column widths proportionally so the total table fits within `width`.
/// Each column gets at least 3 characters. Content is truncated by the caller.
fn fit_columns(natural: &[usize], width: usize) -> Vec<usize> {
    let ncol = natural.len();
    // overhead = 1 (left border) + ncol * 3 (2 padding + 1 border per col)
    let overhead = 1 + ncol * 3;
    let avail = width.saturating_sub(overhead);
    let total_natural: usize = natural.iter().sum();

    if total_natural <= avail || ncol == 0 {
        return natural.to_vec();
    }

    // Distribute proportionally, with a minimum of 3 chars per column.
    const MIN: usize = 3;
    let mut result = vec![0usize; ncol];
    let mut remaining = avail;

    // First pass: assign minimums.
    let min_total = MIN * ncol;
    if min_total >= remaining {
        // Not enough space even for minimums — give everyone MIN.
        return vec![MIN; ncol];
    }
    remaining -= min_total;
    result.fill(MIN);

    // Second pass: distribute the remaining space proportionally to
    // (natural - MIN) for each column.
    let extra_natural: usize = natural.iter().map(|n| n.saturating_sub(MIN)).sum();
    if extra_natural == 0 {
        return result;
    }
    for i in 0..ncol {
        let extra = natural[i].saturating_sub(MIN);
        let share = if i + 1 == ncol {
            remaining // last column gets the remainder (avoids rounding gaps)
        } else {
            remaining * extra / extra_natural
        };
        result[i] += share;
        remaining = remaining.saturating_sub(share);
    }
    result
}

fn make_border(left: char, mid: char, right: char, col_w: &[usize]) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, w) in col_w.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2));
        if i + 1 < col_w.len() {
            s.push(mid);
        }
    }
    s.push(right);
    s
}

fn make_row(
    cells: &[String],
    aligns: &[Alignment],
    col_w: &[usize],
    style: Style,
) -> Line<'static> {
    let bar = Style::default().fg(Color::DarkGray);
    let mut spans = vec![Span::styled("│", bar)];
    for (i, w) in col_w.iter().enumerate() {
        let cell = cells.get(i).map(|s| s.as_str()).unwrap_or("");
        let align = aligns.get(i).copied().unwrap_or(Alignment::None);
        spans.push(Span::styled(
            format!(" {} ", truncate_cell(cell, *w, align)),
            style,
        ));
        spans.push(Span::styled("│", bar));
    }
    Line::from(spans)
}

/// Truncate `cell` to `width` cells, appending `…` if truncated.
/// Then pad to `width` according to alignment.
fn truncate_cell(cell: &str, width: usize, align: Alignment) -> String {
    if width == 0 {
        return String::new();
    }
    let cell_w = width_of(cell);
    if cell_w <= width {
        return pad_cell(cell, width, align);
    }
    // Need to truncate. Reserve 1 cell for the ellipsis.
    let max_content_w = width.saturating_sub(1);
    let mut out = String::new();
    for ch in cell.chars() {
        out.push(ch);
        // Use UnicodeWidthStr (not per-char sum) so emoji variation
        // sequences like ⚠️ are measured correctly as width 2.
        if width_of(&out) > max_content_w {
            out.pop();
            break;
        }
    }
    out.push('…');
    // Pad to fill the remaining space (left/right/center per alignment).
    let out_w = width_of(&out);
    if out_w < width {
        let pad = width - out_w;
        match align {
            Alignment::Right => format!("{}{out}", " ".repeat(pad)),
            Alignment::Center => {
                let left = pad / 2;
                let right = pad - left;
                format!("{}{out}{}", " ".repeat(left), " ".repeat(right))
            }
            Alignment::Left | Alignment::None => format!("{out}{}", " ".repeat(pad)),
        }
    } else {
        out
    }
}

fn pad_cell(cell: &str, width: usize, align: Alignment) -> String {
    let cw = width_of(cell);
    if cw >= width {
        return cell.to_owned();
    }
    let pad = width - cw;
    match align {
        Alignment::Right => format!("{}{cell}", " ".repeat(pad)),
        Alignment::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{cell}{}", " ".repeat(left), " ".repeat(right))
        }
        Alignment::Left | Alignment::None => format!("{cell}{}", " ".repeat(pad)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::mermaid::MermaidRenderer;
    use ratatui::style::Modifier;

    struct OkMermaidRenderer;

    impl MermaidRenderer for OkMermaidRenderer {
        fn render(&self, source: &str) -> Result<String, String> {
            Ok(format!("mock diagram\n{source}"))
        }
    }

    struct ErrMermaidRenderer;

    impl MermaidRenderer for ErrMermaidRenderer {
        fn render(&self, _source: &str) -> Result<String, String> {
            Err("mock failure".to_owned())
        }
    }

    struct LongMermaidRenderer;

    impl MermaidRenderer for LongMermaidRenderer {
        fn render(&self, _source: &str) -> Result<String, String> {
            Ok("0123456789abcdef".to_owned())
        }
    }

    fn plain(line: &Line) -> String {
        let mut s = String::new();
        for span in &line.spans {
            s.push_str(&span.content);
        }
        s
    }

    fn render(md: &str) -> Vec<Line<'static>> {
        render_markdown(md, 80).lines
    }

    fn all_plain(lines: &[Line]) -> String {
        lines.iter().map(plain).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn renders_h1_with_bold_and_color() {
        let lines = render("# Title");
        let h = &lines[0];
        assert!(h.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(h.spans[0].style.fg, Some(Color::Cyan));
        assert!(plain(h).contains("Title"));
    }

    #[test]
    fn renders_all_heading_levels() {
        for (i, level) in ["#", "##", "###", "####", "#####", "######"]
            .iter()
            .enumerate()
        {
            let lines = render(&format!("{level} H"));
            assert!(plain(&lines[0]).contains("H"));
            assert!(
                lines[0].spans[0]
                    .style
                    .add_modifier
                    .contains(Modifier::BOLD)
            );
            let _ = i;
        }
    }

    #[test]
    fn renders_paragraph_and_softbreak() {
        // soft break joins with space -> one line "hello world"
        let lines = render("hello\nworld");
        assert_eq!(lines.len(), 1);
        assert_eq!(plain(&lines[0]), "hello world");
    }

    #[test]
    fn renders_hard_break() {
        // two trailing spaces = hard break -> two lines
        let lines = render("hello  \nworld");
        assert_eq!(lines.len(), 2);
        assert_eq!(plain(&lines[0]), "hello");
        assert_eq!(plain(&lines[1]), "world");
    }

    #[test]
    fn renders_bold_italic_code_strike() {
        let lines = render("**bold** *italic* `code` ~~strike~~");
        let text = all_plain(&lines);
        assert!(text.contains("bold"));
        assert!(text.contains("italic"));
        assert!(text.contains("code"));
        assert!(text.contains("strike"));
        // verify style modifiers are present somewhere
        let has_bold = lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        });
        let has_italic = lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::ITALIC))
        });
        let has_strike = lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::CROSSED_OUT))
        });
        assert!(has_bold && has_italic && has_strike);
    }

    #[test]
    fn inline_code_is_yellow() {
        let lines = render("some `code` here");
        let code_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == "code")
            .unwrap();
        assert_eq!(code_span.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn renders_link_styled_with_url_visible() {
        let lines = render("[example](https://example.com)");
        let text = all_plain(&lines);
        assert!(text.contains("example"));
        assert!(text.contains("https://example.com"));
        let link_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == "example")
            .unwrap();
        assert!(link_span.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(link_span.style.fg, Some(Color::Blue));
    }

    #[test]
    fn renders_fenced_codeblock_with_language_label() {
        let md = "```rust\nfn main() {}\n```";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("rust"));
        assert!(text.contains("fn main() {}"));
    }

    #[test]
    fn detects_mermaid_fenced_block() {
        let md = "```mermaid\ngraph LR\nA-->B\n```";
        let renderer = OkMermaidRenderer;
        let lines = render_markdown_with_mermaid(md, 80, &renderer).lines;
        let text = all_plain(&lines);
        assert!(text.contains("mock diagram"));
        assert!(text.contains("A-->B"));
        assert!(!text.contains("┌─ mermaid"));
    }

    #[test]
    fn mermaid_renderer_trait_is_used_by_markdown_renderer() {
        let md = "before\n\n```mermaid\nsequenceDiagram\nAlice->>Bob: Hi\n```\n\nafter";
        let renderer = OkMermaidRenderer;
        let lines = render_markdown_with_mermaid(md, 80, &renderer).lines;
        let text = all_plain(&lines);
        assert!(text.contains("before"));
        assert!(text.contains("mock diagram"));
        assert!(text.contains("Alice->>Bob: Hi"));
        assert!(text.contains("after"));
    }

    #[test]
    fn rendered_mermaid_is_not_wrapped() {
        let md = "```mermaid\ngraph LR\nA-->B\n```";
        let renderer = LongMermaidRenderer;
        let lines = render_markdown_with_mermaid(md, 80, &renderer).lines;
        assert_eq!(lines.len(), 1);
        assert_eq!(plain(&lines[0]), "0123456789abcdef");
    }

    #[test]
    fn wide_rendered_mermaid_gets_horizontal_scroll_hint() {
        let md = "```mermaid\ngraph LR\nA-->B\n```";
        let renderer = LongMermaidRenderer;
        let lines = render_markdown_with_mermaid(md, 10, &renderer).lines;
        let text = all_plain(&lines);
        assert!(text.contains("Use <-/-> or h/l to pan"));
        assert!(text.contains("0123456789abcdef"));
    }

    #[test]
    fn renders_unsupported_diagram_as_codeblock_fallback() {
        let md = "```mermaid\nunknownDiagram\nA-->B\n```";
        let renderer = ErrMermaidRenderer;
        let lines = render_markdown_with_mermaid(md, 80, &renderer).lines;
        let text = all_plain(&lines);
        assert!(text.contains("┌─ mermaid"));
        assert!(text.contains("unknownDiagram"));
        assert!(text.contains("mermaid render failed: mock failure"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn renders_invalid_mermaid_as_codeblock_fallback() {
        let md = "```mermaid\nnot a diagram\n```";
        let lines = render_markdown(md, 80).lines;
        let text = all_plain(&lines);
        assert!(text.contains("┌─ mermaid"));
        assert!(text.contains("not a diagram"));
        assert!(text.contains("mermaid render failed:"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn problematic_sequence_diagram_does_not_panic() {
        let md = "```mermaid\nsequenceDiagram\n    autonumber\n    actor U as Operator (Atlas-Webapp)\n    participant API as DisbursementController\n    participant DB as Atlas DB<br/>(Disbursement schema)\n    participant HF as Hangfire<br/>(SQL-backed)\n    participant W as DisbursementProcessor<br/>(Hangfire job)\n    participant WL as WorldLink (Citi)\n\n    Note over U,WL: Trigger — returns 202 immediately\n    U->>API: POST /api/disbursement/{id}/initiate-international\n    API->>DB: UPDATE Batch.Status=processing<br/>UPDATE EmployeeDisbursement.Status=queued (N rows)\n    API->>HF: BackgroundJob.Enqueue ProcessBatchAsync(batchId)\n    HF-->>API: hangfireJobId\n    API-->>U: 202 { batchId, hangfireJobId }\n\n    loop for each chunk of 5 (SemaphoreSlim)\n        par employee 1..5 in parallel\n            W->>WL: POST payment/initiation (per-employee pain.001)\n            WL-->>W: 200 paymentId / 4xx validation / 5xx transient\n            alt success\n                Note right of W: Status=submitted\n            else 4xx validation\n                Note right of W: Status=failed_validation (no retry)\n            else 408/429/5xx/network\n                Note right of W: Polly retries 3x w/ backoff<br/>then Status=failed_transient\n            end\n        end\n        W->>W: Task.Delay(1000) pacing\n    end\n```";
        let lines = render_markdown(md, 120).lines;
        let text = all_plain(&lines);
        assert!(text.contains("Operator"));
        assert!(!text.contains("mermaid render failed:"));
    }

    #[test]
    fn renders_indented_codeblock_without_label() {
        let md = "    let x = 1;";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("let x = 1;"));
    }

    #[test]
    fn renders_nested_bullet_list() {
        let md = "- a\n  - b\n  - c\n- d";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("a"));
        assert!(text.contains("b"));
        assert!(text.contains("c"));
        assert!(text.contains("d"));
        // nested items are indented more than top-level
        let a_line = lines.iter().find(|l| plain(l).contains("a")).unwrap();
        let b_line = lines.iter().find(|l| plain(l).contains("b")).unwrap();
        assert!(leading_spaces(&plain(b_line)) > leading_spaces(&plain(a_line)));
    }

    #[test]
    fn renders_ordered_list_with_correct_numbers() {
        let md = "1. first\n2. second\n3. third";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("1. first"));
        assert!(text.contains("2. second"));
        assert!(text.contains("3. third"));
    }

    #[test]
    fn renders_tasklist_checked_and_unchecked() {
        let md = "- [x] done\n- [ ] todo";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("☑"));
        assert!(text.contains("☐"));
        assert!(text.contains("done"));
        assert!(text.contains("todo"));
    }

    #[test]
    fn renders_blockquote_with_left_bar() {
        let md = "> quoted text";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("│"));
        assert!(text.contains("quoted text"));
    }

    #[test]
    fn renders_horizontal_rule() {
        let lines = render("---");
        let text = all_plain(&lines);
        assert!(text.contains("─"));
    }

    #[test]
    fn renders_gfm_table() {
        let md = "| name | age |\n| --- | --- |\n| alice | 30 |\n| bob | 25 |";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("name"));
        assert!(text.contains("age"));
        assert!(text.contains("alice"));
        assert!(text.contains("bob"));
        assert!(text.contains("│"));
    }

    #[test]
    fn wide_table_fits_within_viewport() {
        // A 4-column table with very long content — must fit within 60 chars.
        let md = "| aaaa | bbbb | cccc | dddd |\n| --- | --- | --- | --- |\n| verylongcontenthere | moreverylongcontent | evenlongercontenthere | yetanotherlongcontent |";
        let lines = render_markdown(md, 60).lines;
        for l in &lines {
            let w = width_of(&plain(l));
            assert!(w <= 60, "table line {} chars > 60: [{}]", w, plain(l));
        }
    }

    #[test]
    fn wide_table_truncates_with_ellipsis() {
        let md = "| short | averyveryveryverylongcell |\n| --- | --- |\n| x | y |";
        let lines = render_markdown(md, 30).lines;
        let text = all_plain(&lines);
        // The long cell should be truncated (contains ellipsis somewhere)
        assert!(
            text.contains('…'),
            "expected ellipsis in truncated table: {text}"
        );
    }

    #[test]
    fn table_with_emoji_variation_sequence_fits_width() {
        // Regression: ⚠️ (U+26A0 + U+FE0F) is width 2 via UnicodeWidthStr
        // but width 1 if summed per-char. Tables must use str-level width
        // so cells with emoji don't overflow the column.
        let md = "| a | b |\n| --- | --- |\n| ⚠️ warning | ✅ ok |";
        let lines = render_markdown(md, 30).lines;
        for l in &lines {
            let s: String = l.spans.iter().map(|sp| sp.content.as_ref()).collect();
            let w = unicode_width::UnicodeWidthStr::width(s.as_str());
            assert!(w <= 30, "table line {w} > 30: [{s}]");
        }
    }

    #[test]
    fn link_inside_table_cell_keeps_url_in_cell() {
        // Regression: link URL was orphaned outside the table because
        // End(Link) always pushed to self.pending instead of self.cell_buf.
        let md = "| name | link |\n| --- | --- |\n| alice | [home](https://example.com) |";
        let lines = render_markdown(md, 200).lines;
        let text = all_plain(&lines);
        // URL must appear inside the table, not as a stray line after it.
        assert!(text.contains("https://example.com"));
        // The table border should be the last non-empty line (no orphan).
        let last_non_empty = lines.iter().rev().find(|l| {
            let s: String = l.spans.iter().map(|sp| sp.content.as_ref()).collect();
            !s.trim().is_empty()
        });
        let last_text: String = last_non_empty
            .unwrap()
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(
            last_text.contains('└'),
            "table should end with bottom border, got: [{last_text}]"
        );
    }

    #[test]
    fn renders_image_as_alt_text_plus_url() {
        let md = "![logo](https://example.com/logo.png)";
        let lines = render(md);
        let text = all_plain(&lines);
        assert!(text.contains("logo"));
        assert!(text.contains("https://example.com/logo.png"));
    }

    #[test]
    fn skips_or_escapes_raw_html() {
        let md = "<b>bold</b>";
        let lines = render(md);
        let text = all_plain(&lines);
        // raw html is shown as literal escaped text (dim), not interpreted
        assert!(text.contains("<b>"));
        assert!(text.contains("</b>"));
    }

    #[test]
    fn empty_markdown_yields_no_lines() {
        let lines = render("");
        assert!(lines.is_empty());
    }

    #[test]
    fn heading_index_captures_levels_and_text() {
        let md = "# Title\n\n## Sub\n\n### Deep\n\nText\n\n# Another";
        let out = render_markdown(md, 80);
        assert_eq!(out.headings.len(), 4);
        assert_eq!(out.headings[0].level, 1);
        assert_eq!(out.headings[0].text, "Title");
        assert_eq!(out.headings[1].level, 2);
        assert_eq!(out.headings[1].text, "Sub");
        assert_eq!(out.headings[2].level, 3);
        assert_eq!(out.headings[2].text, "Deep");
        assert_eq!(out.headings[3].level, 1);
        assert_eq!(out.headings[3].text, "Another");
        // Heading line indices must point to the heading's first row.
        assert_eq!(plain(&out.lines[out.headings[0].line]), "Title");
        assert_eq!(plain(&out.lines[out.headings[1].line]), "Sub");
    }

    #[test]
    fn heading_index_is_empty_for_plain_text() {
        use crate::render::text::render_text;
        let out = render_text("not a heading\n\n## not either", 80, false);
        assert!(out.headings.is_empty());
    }

    #[test]
    fn paragraph_wraps_at_narrow_width() {
        let lines = render_markdown("hello world foo bar baz", 10).lines;
        // wrapped into multiple lines, none wider than 10
        for l in &lines {
            assert!(width_of(&plain(l)) <= 10, "line too wide: {:?}", plain(l));
        }
        assert!(lines.len() > 1);
    }

    fn leading_spaces(s: &str) -> usize {
        s.chars().take_while(|c| *c == ' ').count()
    }
}
