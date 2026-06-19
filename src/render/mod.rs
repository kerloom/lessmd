//! Render dispatch: turns an [`Input`] into a flat `Vec<Line>` wrapped to width.

use ratatui::text::Line;

use crate::source::{Input, ResolvedMode};

pub mod markdown;
pub mod mermaid;
pub mod text;

/// Render the whole document up-front to a flat list of terminal rows,
/// already wrapped to `width`. The pager just slices a window over this.
pub fn render(input: &Input, width: u16) -> Vec<Line<'static>> {
    match input.render_mode {
        ResolvedMode::Text { ansi } => text::render_text(&input.text, width, ansi),
        ResolvedMode::Markdown => markdown::render_markdown(&input.text, width),
    }
}
