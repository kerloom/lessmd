//! Resolve input: read a file or stdin, decide the render mode.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::cli::RenderMode;

#[derive(Debug, Clone)]
pub struct Input {
    pub text: String,
    pub render_mode: ResolvedMode,
    pub source_path: Option<PathBuf>,
}

/// `Auto` is resolved into a concrete mode by [`read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedMode {
    /// Plain text. `ansi` = interpret SGR escape sequences as styles.
    Text { ansi: bool },
    /// Render markdown (M2).
    Markdown,
}

pub fn read(path: Option<&Path>, mode: RenderMode) -> io::Result<Input> {
    let (text, source_path) = match path {
        Some(p) => {
            let bytes = std::fs::read(p)?;
            (
                String::from_utf8_lossy(&bytes).into_owned(),
                Some(p.to_path_buf()),
            )
        }
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            (String::from_utf8_lossy(&buf).into_owned(), None)
        }
    };
    let render_mode = resolve_mode(path, mode);
    Ok(Input {
        text,
        render_mode,
        source_path,
    })
}

fn resolve_mode(path: Option<&Path>, mode: RenderMode) -> ResolvedMode {
    match mode {
        RenderMode::Markdown => ResolvedMode::Markdown,
        RenderMode::Plain => ResolvedMode::Text { ansi: false },
        RenderMode::Auto => {
            let is_md = path
                .and_then(|p| p.extension())
                .map(|e| e == "md" || e == "markdown")
                .unwrap_or(false);
            if is_md {
                ResolvedMode::Markdown
            } else {
                ResolvedMode::Text { ansi: true }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detects_markdown_extension() {
        assert_eq!(
            resolve_mode(Some(Path::new("x.md")), RenderMode::Auto),
            ResolvedMode::Markdown
        );
        assert_eq!(
            resolve_mode(Some(Path::new("x.markdown")), RenderMode::Auto),
            ResolvedMode::Markdown
        );
        assert_eq!(
            resolve_mode(Some(Path::new("x.txt")), RenderMode::Auto),
            ResolvedMode::Text { ansi: true }
        );
    }

    #[test]
    fn auto_without_path_defaults_to_text_ansi() {
        assert_eq!(
            resolve_mode(None, RenderMode::Auto),
            ResolvedMode::Text { ansi: true }
        );
    }

    #[test]
    fn plain_strips_ansi() {
        assert_eq!(
            resolve_mode(Some(Path::new("x.md")), RenderMode::Plain),
            ResolvedMode::Text { ansi: false }
        );
    }

    #[test]
    fn markdown_flag_overrides_extension() {
        assert_eq!(
            resolve_mode(Some(Path::new("x.txt")), RenderMode::Markdown),
            ResolvedMode::Markdown
        );
    }
}
