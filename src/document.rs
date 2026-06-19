//! The rendered document: a flat `Vec<Line>` plus its source path.

use std::path::PathBuf;

use ratatui::text::Line;

use crate::render;
use crate::source::Input;

/// A heading captured during markdown rendering, used for the outline
/// overlay and jump-to-heading navigation.
#[derive(Debug, Clone)]
pub struct Heading {
    /// Heading level (1–6).
    pub level: u8,
    /// Plain-text heading content.
    pub text: String,
    /// Index into [`Document::lines`] where the heading's first row lives.
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<Heading>,
    pub source_path: Option<PathBuf>,
}

impl Document {
    /// Render the whole `input` up-front, wrapped to `width`.
    pub fn new(input: &Input, width: u16) -> Self {
        let output = render::render(input, width);
        Self {
            lines: output.lines,
            headings: output.headings,
            source_path: input.source_path.clone(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Return up to `height` consecutive lines starting at `offset`,
    /// clamped to the document bounds.
    pub fn slice(&self, offset: usize, height: usize) -> &[Line<'static>] {
        let start = offset.min(self.lines.len());
        let end = (start + height).min(self.lines.len());
        &self.lines[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ResolvedMode;

    fn input(text: &str) -> Input {
        Input {
            text: text.to_owned(),
            render_mode: ResolvedMode::Text { ansi: false },
            source_path: None,
        }
    }

    #[test]
    fn slice_clamps_to_bounds() {
        let doc = Document::new(&input("a\nb\nc\nd\ne"), 80);
        assert_eq!(doc.line_count(), 5);
        assert_eq!(doc.slice(0, 3).len(), 3);
        assert_eq!(doc.slice(3, 10).len(), 2); // clamps at end
        assert_eq!(doc.slice(10, 3).len(), 0); // past end
    }

    #[test]
    fn empty_document() {
        let doc = Document::new(&input(""), 80);
        assert_eq!(doc.line_count(), 0);
        assert_eq!(doc.slice(0, 10).len(), 0);
    }
}
