//! Hand-rolled argument parsing (no clap, to keep deps minimal).

use std::path::PathBuf;

pub use crate::pager::HighlightMode;
pub use crate::search::CaseMode;

#[derive(Debug, Clone, Default)]
pub struct Args {
    pub path: Option<PathBuf>,
    pub mode: RenderMode,
    pub line_numbers: bool,
    pub syntax: bool,
    pub mermaid: bool,
    pub show_help: bool,
    pub show_version: bool,
    /// `-F` / `--quit-if-one-screen`: exit immediately if the entire file
    /// fits on the first screen.
    pub quit_if_one_screen: bool,
    /// `-i` / `-I`: case-sensitivity mode for `/` searches. Mirrors `less`.
    pub case_mode: CaseMode,
    /// `-g` / `-G`: search-match highlight mode. Mirrors `less`.
    pub highlight: HighlightMode,
}

impl Args {
    fn new() -> Self {
        Self {
            syntax: true,
            mermaid: true,
            ..Self::default()
        }
    }
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
  --no-syntax       Disable fenced code syntax highlighting.
  --no-mermaid      Disable inline Mermaid rendering.
  -N, --line-numbers  Show line numbers in a left gutter.
  -F, --quit-if-one-screen  Exit if the whole file fits on the first screen.
  -i, --ignore-case  Ignore case in searches (unless pattern has uppercase).
  -I, --IGNORE-CASE  Ignore case in all searches.
  -g, --hilite-search  Highlight only the current search match.
  -G, --HILITE-SEARCH  Suppress all search-match highlighting.
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
  r / Ctrl-L               repaint (no-op; ratatui redraws every frame)
  Esc-u                    toggle search-match highlighting
  Esc-U                    clear saved search pattern + highlighting
  Ctrl-C                   abort search
  ?                        toggle help
  q / Q / Esc              quit
";

/// Parse program arguments (the iterator must include the program name as
/// the first element, as returned by `std::env::args`).
pub fn parse<I: Iterator<Item = String>>(args: I) -> Result<Args, String> {
    let args = args.skip(1);
    let mut out = Args::new();
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
            "--no-syntax" => out.syntax = false,
            "--no-mermaid" => out.mermaid = false,
            "-N" | "--line-numbers" => out.line_numbers = true,
            "-F" | "--quit-if-one-screen" => out.quit_if_one_screen = true,
            "-i" | "--ignore-case" => out.case_mode = CaseMode::Smart,
            "-I" | "--IGNORE-CASE" => out.case_mode = CaseMode::Insensitive,
            "-g" | "--hilite-search" => out.highlight = HighlightMode::Last,
            "-G" | "--HILITE-SEARCH" => out.highlight = HighlightMode::None,
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
    fn syntax_and_mermaid_default_on() {
        let a = parse_args(&[]);
        assert!(a.syntax);
        assert!(a.mermaid);
    }

    #[test]
    fn no_syntax_and_no_mermaid_flags() {
        let a = parse_args(&["--no-syntax", "--no-mermaid", "x.md"]);
        assert!(!a.syntax);
        assert!(!a.mermaid);
    }

    #[test]
    fn help_and_version_flags() {
        assert!(parse_args(&["--help"]).show_help);
        assert!(parse_args(&["-h"]).show_help);
        assert!(parse_args(&["--version"]).show_version);
        assert!(parse_args(&["-V"]).show_version);
    }

    #[test]
    fn quit_if_one_screen_flag() {
        assert!(parse_args(&["-F", "x"]).quit_if_one_screen);
        assert!(parse_args(&["--quit-if-one-screen", "x"]).quit_if_one_screen);
        assert!(!parse_args(&["x"]).quit_if_one_screen);
    }

    #[test]
    fn ignore_case_flags() {
        use CaseMode::*;
        assert_eq!(parse_args(&[]).case_mode, Sensitive);
        assert_eq!(parse_args(&["-i", "x"]).case_mode, Smart);
        assert_eq!(parse_args(&["--ignore-case", "x"]).case_mode, Smart);
        assert_eq!(parse_args(&["-I", "x"]).case_mode, Insensitive);
        assert_eq!(parse_args(&["--IGNORE-CASE", "x"]).case_mode, Insensitive);
    }

    #[test]
    fn highlight_mode_flags() {
        use HighlightMode::*;
        assert_eq!(parse_args(&[]).highlight, All);
        assert_eq!(parse_args(&["-g", "x"]).highlight, Last);
        assert_eq!(parse_args(&["--hilite-search", "x"]).highlight, Last);
        assert_eq!(parse_args(&["-G", "x"]).highlight, None);
        assert_eq!(parse_args(&["--HILITE-SEARCH", "x"]).highlight, None);
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
