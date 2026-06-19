//! Hand-rolled argument parsing (no clap, to keep deps minimal).

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Args {
    pub path: Option<PathBuf>,
    pub mode: RenderMode,
    pub line_numbers: bool,
    pub show_help: bool,
    pub show_version: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// Detect by file extension (`.md`/`.markdown` -> markdown, else text).
    #[default]
    Auto,
    /// Force markdown rendering.
    Markdown,
    /// Force plain-text rendering and strip ANSI colors.
    Plain,
}

pub const HELP_TEXT: &str = "\
lessmd — a less-like terminal pager that renders markdown and mermaid.

Usage:
  lessmd [OPTIONS] [FILE]
  lessmd [OPTIONS] -         (read from stdin)

Options:
  --markdown        Force markdown rendering (ignore file extension).
  --plain           Force plain-text rendering and strip ANSI colors.
  -N, --line-numbers  Show line numbers in a left gutter.
  -h, --help        Show this help text and exit.
  -V, --version     Show version and exit.

When FILE is omitted or '-', lessmd reads from stdin.

Keybindings (inside the pager):
  j / e / Down             scroll down one line
  k / y / Up               scroll up one line
  h / Left                 pan left
  l / Right                pan right
  Space / f / PageDown     scroll down one page
  b / PageUp               scroll up one page
  Ctrl-D                   scroll down half a page
  Ctrl-U                   scroll up half a page
  g / Home                 go to top
  G / End                  go to bottom
  t                        next heading
  T                        previous heading
  o                        toggle outline (jump to heading)
  Tab                      toggle fold on heading
  /                        start search
  n                        next search match
  N                        previous search match
  Ctrl-C                   abort search
  ?                        toggle help
  q / Q / Esc              quit
";

/// Parse program arguments (the iterator must include the program name as
/// the first element, as returned by `std::env::args`).
pub fn parse<I: Iterator<Item = String>>(args: I) -> Result<Args, String> {
    let args = args.skip(1);
    let mut out = Args::default();
    let mut positional: Vec<String> = Vec::new();
    let mut only_positional = false;

    for arg in args {
        if only_positional {
            positional.push(arg);
            continue;
        }
        match arg.as_str() {
            "--markdown" => out.mode = RenderMode::Markdown,
            "--plain" => out.mode = RenderMode::Plain,
            "-N" | "--line-numbers" => out.line_numbers = true,
            "-h" | "--help" => out.show_help = true,
            "-V" | "--version" => out.show_version = true,
            "--" => only_positional = true,
            "-" => positional.push("-".to_owned()),
            s if s.starts_with("--") => return Err(format!("unknown option: {s}")),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option: {s}")),
            s => positional.push(s.to_owned()),
        }
    }

    match positional.len() {
        0 => out.path = None,
        1 => {
            if positional[0] == "-" {
                out.path = None;
            } else {
                out.path = Some(PathBuf::from(&positional[0]));
            }
        }
        n => return Err(format!("expected at most one file, got {n}")),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Args {
        let argv: Vec<String> = std::iter::once("lessmd".to_owned())
            .chain(args.iter().copied().map(String::from))
            .collect();
        parse(argv.into_iter()).unwrap()
    }

    #[test]
    fn no_args_reads_stdin() {
        let a = parse_args(&[]);
        assert!(a.path.is_none());
        assert_eq!(a.mode, RenderMode::Auto);
    }

    #[test]
    fn dash_means_stdin() {
        let a = parse_args(&["-"]);
        assert!(a.path.is_none());
    }

    #[test]
    fn file_path_recorded() {
        let a = parse_args(&["foo.txt"]);
        assert_eq!(a.path, Some(PathBuf::from("foo.txt")));
    }

    #[test]
    fn markdown_and_plain_flags() {
        assert_eq!(parse_args(&["--markdown", "x"]).mode, RenderMode::Markdown);
        assert_eq!(parse_args(&["--plain", "x"]).mode, RenderMode::Plain);
    }

    #[test]
    fn line_numbers_flag() {
        assert!(parse_args(&["-N", "x"]).line_numbers);
        assert!(parse_args(&["--line-numbers", "x"]).line_numbers);
        assert!(!parse_args(&["x"]).line_numbers);
    }

    #[test]
    fn help_and_version_flags() {
        assert!(parse_args(&["--help"]).show_help);
        assert!(parse_args(&["-h"]).show_help);
        assert!(parse_args(&["--version"]).show_version);
        assert!(parse_args(&["-V"]).show_version);
    }

    #[test]
    fn double_dash_treats_rest_as_positional() {
        let a = parse_args(&["--", "--weird-file-name"]);
        assert_eq!(a.path, Some(PathBuf::from("--weird-file-name")));
    }

    #[test]
    fn unknown_option_errors() {
        let argv = vec!["lessmd".to_owned(), "--nope".to_owned()];
        assert!(parse(argv.into_iter()).is_err());
    }
}
