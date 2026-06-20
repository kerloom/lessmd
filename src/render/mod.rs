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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub syntax: bool,
    pub mermaid: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            syntax: cfg!(feature = "syntax"),
            mermaid: cfg!(feature = "mermaid"),
        }
    }
}

/// Result of rendering: the pre-wrapped lines plus any captured headings.
pub struct RenderOutput {
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<Heading>,
}

/// Render the whole document up-front to a flat list of terminal rows,
/// already wrapped to `width`. The pager just slices a window over this.
pub fn render(input: &Input, width: u16) -> RenderOutput {
    render_with_options(input, width, RenderOptions::default())
}

pub fn render_with_options(input: &Input, width: u16, options: RenderOptions) -> RenderOutput {
    match input.render_mode {
        ResolvedMode::Text { ansi } => text::render_text(&input.text, width, ansi),
        ResolvedMode::Markdown => {
            markdown::render_markdown_with_options(&input.text, width, options)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_options_defaults_match_compiled_features() {
        let options = RenderOptions::default();
        assert_eq!(options.syntax, cfg!(feature = "syntax"));
        assert_eq!(options.mermaid, cfg!(feature = "mermaid"));
    }
}
