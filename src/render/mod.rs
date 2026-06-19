//! Render dispatch: turns an [`Input`] into a flat `Vec<Line>` wrapped to width,
//! plus a list of headings (empty for plain-text mode).

use ratatui::text::Line;

use crate::document::Heading;
use crate::source::{Input, ResolvedMode};

pub mod markdown;
pub mod mermaid;
#[cfg(feature = "syntax")]
pub mod syntax;
pub mod text;

/// Result of rendering: the pre-wrapped lines plus any captured headings.
pub struct RenderOutput {
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<Heading>,
}

/// Render the whole document up-front to a flat list of terminal rows,
/// already wrapped to `width`. The pager just slices a window over this.
pub fn render(input: &Input, width: u16) -> RenderOutput {
    match input.render_mode {
        ResolvedMode::Text { ansi } => text::render_text(&input.text, width, ansi),
        ResolvedMode::Markdown => markdown::render_markdown(&input.text, width),
    }
}
