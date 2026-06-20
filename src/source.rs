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

/// Guardrail against accidentally loading unbounded stdin or enormous files
/// into memory. Rendering is still whole-document, so fail before allocating
/// pathological inputs.
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;

pub fn read(path: Option<&Path>, mode: RenderMode) -> io::Result<Input> {
    read_with_limit(path, mode, DEFAULT_MAX_INPUT_BYTES)
}

fn read_with_limit(path: Option<&Path>, mode: RenderMode, max_bytes: u64) -> io::Result<Input> {
    let (text, source_path) = match path {
        Some(p) => {
            if std::fs::metadata(p)?.len() > max_bytes {
                return Err(input_too_large(max_bytes));
            }
            let bytes = read_bytes_limited(std::fs::File::open(p)?, max_bytes)?;
            (
                String::from_utf8_lossy(&bytes).into_owned(),
                Some(p.to_path_buf()),
            )
        }
        None => {
            let buf = read_bytes_limited(io::stdin(), max_bytes)?;
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

fn read_bytes_limited<R: Read>(reader: R, max_bytes: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        return Err(input_too_large(max_bytes));
    }
    Ok(buf)
}

fn input_too_large(max_bytes: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("input exceeds {max_bytes} byte limit"),
    )
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
    use std::io::Cursor;

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

    #[test]
    fn limited_reader_accepts_input_at_limit() {
        let bytes = read_bytes_limited(Cursor::new(b"abcd"), 4).unwrap();
        assert_eq!(bytes, b"abcd");
    }

    #[test]
    fn limited_reader_rejects_input_over_limit() {
        let err = read_bytes_limited(Cursor::new(b"abcde"), 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("4 byte limit"));
    }

    #[test]
    fn read_file_rejects_metadata_over_limit() {
        let path = std::env::temp_dir().join(format!(
            "lessmd-source-limit-{}-{}.txt",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, b"abcde").unwrap();

        let err = read_with_limit(Some(&path), RenderMode::Auto, 4).unwrap_err();

        let _ = std::fs::remove_file(&path);
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("4 byte limit"));
    }
}
