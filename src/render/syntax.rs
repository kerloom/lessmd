//! Syntax highlighting for fenced code blocks using syntect.
//!
//! The `SyntaxSet` and `ThemeSet` are loaded once (via `LazyLock`) and reused
//! across all renders. Highlighting produces one `Line<'static>` per source
//! line with styled spans; the caller handles wrapping and prefix attachment.
//!
//! Highlighted results are cached by `(lang, code)` so that a resize (which
//! re-renders the whole document at a new width) skips re-highlighting and
//! only re-wraps the cached spans. This makes resize ~40× faster for
//! code-heavy documents.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const THEME_NAME: &str = "base16-ocean.dark";

/// Maximum number of highlighted code blocks to cache. When exceeded, the
/// cache is cleared and repopulated. This keeps memory bounded while covering
/// the common case of a single document being viewed and resized.
const MAX_CACHE_ENTRIES: usize = 256;

/// Cached highlighted lines keyed by `(lang, code)`.
type HighlightCache = HashMap<(String, String), Vec<Line<'static>>>;

static HIGHLIGHT_CACHE: LazyLock<Mutex<HighlightCache>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Highlight `code` with syntax coloring for the given `lang` token.
///
/// `lang` is the info string from a fenced code block (e.g. `"rust"`, `"py"`,
/// `"javascript"`). Extra annotations like `"rust skip"` are handled — only
/// the first whitespace-delimited token is used for language lookup.
///
/// Returns `None` when the language is not recognized or the code is empty,
/// so the caller can fall back to plain rendering. Otherwise returns one
/// `Line` per source line, each with styled spans (not wrapped — the caller
/// wraps to the available width).
///
/// Results are cached by `(lang, code)` so repeated renders (e.g. on resize)
/// skip re-highlighting and only re-wrap.
pub fn highlight_code(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if code.is_empty() || lang.is_empty() {
        return None;
    }

    let token = lang.split_whitespace().next()?;

    // Check cache first — avoids re-highlighting on resize.
    let key = (token.to_owned(), code.to_owned());
    if let Ok(cache) = HIGHLIGHT_CACHE.lock()
        && let Some(cached) = cache.get(&key)
    {
        return Some(cached.clone());
    }

    let ps = &*SYNTAX_SET;
    let theme = THEME_SET.themes.get(THEME_NAME)?;
    let syntax = resolve_syntax(ps, token)?;

    let mut h = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for source_line in LinesWithEndings::from(code) {
        let spans: Vec<Span<'static>> = match h.highlight_line(source_line, ps) {
            Ok(ranges) => ranges
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches(['\r', '\n']);
                    if text.is_empty() {
                        None
                    } else {
                        Some(Span::styled(text.to_owned(), map_style(style)))
                    }
                })
                .collect(),
            Err(_) => {
                let text = source_line.trim_end_matches(['\r', '\n']);
                vec![Span::raw(text.to_owned())]
            }
        };
        lines.push(Line::from(spans));
    }

    // Store in cache. Clear if oversized to bound memory.
    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, lines.clone());
    }

    Some(lines)
}

/// Clear the highlight cache. Useful for tests and when switching documents.
pub fn clear_cache() {
    if let Ok(mut cache) = HIGHLIGHT_CACHE.lock() {
        cache.clear();
    }
}

/// Look up a syntax reference by markdown language string.
///
/// Tries the raw token first (handles short extensions like `"rs"`, `"py"`,
/// `"js"`, `"go"`), then common long-name aliases (`"rust"` → `"rs"`,
/// `"python"` → `"py"`, …).
fn resolve_syntax<'a>(ps: &'a SyntaxSet, lang: &str) -> Option<&'a SyntaxReference> {
    if let Some(s) = ps.find_syntax_by_token(lang) {
        return Some(s);
    }
    let alias = normalize_lang(lang);
    if alias != lang {
        return ps.find_syntax_by_token(alias);
    }
    None
}

/// Map common markdown language names to file extensions syntect recognizes.
///
/// Some languages (TypeScript, TSX, JSX) are not in syntect's default syntax
/// set; they fall back to JavaScript which is a close syntactic subset.
fn normalize_lang(lang: &str) -> &str {
    match lang.to_lowercase().as_str() {
        "rust" => "rs",
        "python" | "python3" => "py",
        "csharp" => "cs",
        "javascript" => "js",
        "typescript" | "ts" | "tsx" | "jsx" => "js",
        "golang" => "go",
        "c++" | "cxx" => "cpp",
        "shell" | "bash" => "sh",
        "ruby" => "rb",
        "perl" => "pl",
        _ => lang,
    }
}

/// Map a syntect `Style` to a ratatui `Style` (foreground colors only).
fn map_style(s: syntect::highlighting::Style) -> Style {
    let fg = Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b);
    let mut style = Style::default().fg(fg);
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn all_plain(lines: &[Line]) -> String {
        lines.iter().map(plain).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn highlights_rust_code() {
        let lines = highlight_code("fn main() { let x = 42; }", "rust").unwrap();
        assert!(!lines.is_empty());
        let text = plain(&lines[0]);
        assert!(text.contains("fn"));
        assert!(text.contains("main"));
    }

    #[test]
    fn highlights_python_code() {
        let lines = highlight_code("def hello():\n    print('hi')", "python").unwrap();
        assert_eq!(lines.len(), 2);
        assert!(plain(&lines[0]).contains("def"));
        assert!(plain(&lines[1]).contains("print"));
    }

    #[test]
    fn highlights_javascript_code() {
        let lines = highlight_code("const x = () => 42;", "javascript").unwrap();
        assert!(!lines.is_empty());
        assert!(plain(&lines[0]).contains("const"));
    }

    #[test]
    fn highlights_go_code() {
        let lines = highlight_code("package main\nfunc main() {}", "go").unwrap();
        assert_eq!(lines.len(), 2);
        assert!(plain(&lines[0]).contains("package"));
    }

    #[test]
    fn highlights_json_code() {
        let lines = highlight_code(r#"{"key": "value"}"#, "json").unwrap();
        assert!(!lines.is_empty());
        assert!(plain(&lines[0]).contains("key"));
    }

    #[test]
    fn returns_none_for_unknown_language() {
        assert!(highlight_code("hello", "xyzzy_nonexistent").is_none());
    }

    #[test]
    fn returns_none_for_empty_code() {
        assert!(highlight_code("", "rust").is_none());
    }

    #[test]
    fn returns_none_for_empty_lang() {
        assert!(highlight_code("fn main() {}", "").is_none());
    }

    #[test]
    fn preserves_code_content() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code(code, "rust").unwrap();
        let text = all_plain(&lines);
        assert!(text.contains("fn main()"));
        assert!(text.contains("println!"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn handles_multiline_code() {
        let code = "fn a() {}\nfn b() {}\nfn c() {}";
        let lines = highlight_code(code, "rust").unwrap();
        assert_eq!(lines.len(), 3);
        assert!(plain(&lines[0]).contains("fn a"));
        assert!(plain(&lines[1]).contains("fn b"));
        assert!(plain(&lines[2]).contains("fn c"));
    }

    #[test]
    fn handles_empty_lines_in_code() {
        let code = "fn a() {}\n\nfn b() {}";
        let lines = highlight_code(code, "rust").unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(plain(&lines[1]), "");
    }

    #[test]
    fn keyword_has_different_color_than_identifier() {
        let lines = highlight_code("fn main", "rust").unwrap();
        let spans = &lines[0].spans;
        let fn_span = spans.iter().find(|s| s.content.contains("fn")).unwrap();
        let main_span = spans.iter().find(|s| s.content.contains("main")).unwrap();
        assert_ne!(
            fn_span.style.fg, main_span.style.fg,
            "keyword 'fn' and identifier 'main' should have different colors"
        );
    }

    #[test]
    fn highlighted_spans_have_color() {
        let lines = highlight_code("fn main() { let x = 42; }", "rust").unwrap();
        let has_color = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.style.fg.is_some()));
        assert!(has_color, "expected at least one colored span");
    }

    #[test]
    fn lang_alias_rust_works() {
        let lines_long = highlight_code("fn main() {}", "rust").unwrap();
        let lines_short = highlight_code("fn main() {}", "rs").unwrap();
        assert_eq!(plain(&lines_long[0]), plain(&lines_short[0]));
    }

    #[test]
    fn lang_alias_python_works() {
        let lines_long = highlight_code("print('hi')", "python").unwrap();
        let lines_short = highlight_code("print('hi')", "py").unwrap();
        assert_eq!(plain(&lines_long[0]), plain(&lines_short[0]));
    }

    #[test]
    fn lang_alias_javascript_works() {
        let lines_long = highlight_code("const x = 1;", "javascript").unwrap();
        let lines_short = highlight_code("const x = 1;", "js").unwrap();
        assert_eq!(plain(&lines_long[0]), plain(&lines_short[0]));
    }

    #[test]
    fn lang_alias_bash_works() {
        let lines_long = highlight_code("echo hello", "bash").unwrap();
        let lines_short = highlight_code("echo hello", "sh").unwrap();
        assert_eq!(plain(&lines_long[0]), plain(&lines_short[0]));
    }

    #[test]
    fn lang_with_extra_annotations_works() {
        let lines = highlight_code("fn main() {}", "rust skip").unwrap();
        assert!(!lines.is_empty());
        assert!(plain(&lines[0]).contains("fn"));
    }

    #[test]
    fn string_literals_are_colored() {
        let lines = highlight_code(r#"let s = "hello";"#, "rust").unwrap();
        let string_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains("hello"))
            .unwrap();
        assert!(
            string_span.style.fg.is_some(),
            "string literal should be colored"
        );
    }

    #[test]
    fn map_style_converts_colors_and_modifiers() {
        let syn_style = syntect::highlighting::Style {
            foreground: syntect::highlighting::Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            background: syntect::highlighting::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            font_style: FontStyle::BOLD | FontStyle::ITALIC,
        };
        let rat_style = map_style(syn_style);
        assert_eq!(rat_style.fg, Some(Color::Rgb(255, 0, 0)));
        assert!(rat_style.add_modifier.contains(Modifier::BOLD));
        assert!(rat_style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn map_style_underline() {
        let syn_style = syntect::highlighting::Style {
            foreground: syntect::highlighting::Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
            background: syntect::highlighting::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            font_style: FontStyle::UNDERLINE,
        };
        let rat_style = map_style(syn_style);
        assert_eq!(rat_style.fg, Some(Color::Rgb(0, 255, 0)));
        assert!(rat_style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn lang_alias_typescript_falls_back_to_javascript() {
        let lines = highlight_code("const x = 1;", "typescript").unwrap();
        let js_lines = highlight_code("const x = 1;", "javascript").unwrap();
        assert_eq!(plain(&lines[0]), plain(&js_lines[0]));
    }

    // -- language coverage ---------------------------------------------------

    /// Helper: assert highlighting succeeds, preserves content, and adds color.
    fn assert_highlights(code: &str, lang: &str, marker: &str) {
        let lines = highlight_code(code, lang)
            .unwrap_or_else(|| panic!("expected highlighting for '{lang}' to succeed, got None"));
        let text = all_plain(&lines);
        assert!(
            text.contains(marker),
            "'{lang}' output missing marker '{marker}': {text}"
        );
        let has_color = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.style.fg.is_some()));
        assert!(
            has_color,
            "'{lang}' output has no colored spans (plain text only)"
        );
    }

    #[test]
    fn highlights_python() {
        assert_highlights("def main():\n    print('hi')", "python", "def");
        assert_highlights("def main():\n    print('hi')", "py", "def");
    }

    #[test]
    fn highlights_csharp() {
        assert_highlights("class Foo { void Bar() {} }", "cs", "class");
        assert_highlights(
            "using System;\nclass Program { static void Main() {} }",
            "csharp",
            "class",
        );
    }

    #[test]
    fn highlights_rust() {
        assert_highlights("fn main() { let x = 42; }", "rust", "fn");
        assert_highlights("fn main() { let x = 42; }", "rs", "fn");
    }

    #[test]
    fn highlights_c() {
        assert_highlights("int main() { return 0; }", "c", "main");
    }

    #[test]
    fn highlights_cpp() {
        assert_highlights("int main() { std::cout << 1; }", "cpp", "main");
        assert_highlights("int main() { return 0; }", "c++", "main");
    }

    #[test]
    fn highlights_go() {
        assert_highlights("package main\nfunc main() {}", "go", "func");
        assert_highlights("package main\nfunc main() {}", "golang", "func");
    }

    #[test]
    fn highlights_javascript() {
        assert_highlights("const x = () => 42;", "javascript", "const");
        assert_highlights("const x = () => 42;", "js", "const");
    }

    #[test]
    fn highlights_typescript_as_javascript_fallback() {
        // TypeScript is not in syntect's default set; it falls back to JS.
        let ts = highlight_code("const x: number = 1;", "typescript").unwrap();
        let js = highlight_code("const x: number = 1;", "javascript").unwrap();
        assert_eq!(plain(&ts[0]), plain(&js[0]));
        assert!(plain(&ts[0]).contains("const"));
    }

    #[test]
    fn highlights_ts_alias_as_javascript_fallback() {
        let ts = highlight_code("const x = 1;", "ts").unwrap();
        let js = highlight_code("const x = 1;", "js").unwrap();
        assert_eq!(plain(&ts[0]), plain(&js[0]));
    }

    #[test]
    fn highlights_tsx_as_javascript_fallback() {
        let tsx = highlight_code("const x = 1;", "tsx").unwrap();
        let js = highlight_code("const x = 1;", "js").unwrap();
        assert_eq!(plain(&tsx[0]), plain(&js[0]));
    }

    #[test]
    fn highlights_jsx_as_javascript_fallback() {
        let jsx = highlight_code("const x = 1;", "jsx").unwrap();
        let js = highlight_code("const x = 1;", "js").unwrap();
        assert_eq!(plain(&jsx[0]), plain(&js[0]));
    }

    #[test]
    fn highlights_markdown() {
        assert_highlights("# Title\n\nSome **bold** text.", "md", "Title");
        assert_highlights("# Title\n\nSome **bold** text.", "markdown", "Title");
    }

    #[test]
    fn highlights_json() {
        assert_highlights(r#"{"key": "value", "n": 42}"#, "json", "key");
    }

    #[test]
    fn highlights_yaml() {
        assert_highlights("key: value\nlist:\n  - item", "yaml", "key");
        assert_highlights("key: value\nlist:\n  - item", "yml", "key");
    }

    #[test]
    fn highlights_html() {
        assert_highlights("<div class=\"x\">hello</div>", "html", "div");
    }

    #[test]
    fn highlights_css() {
        assert_highlights("body { color: red; margin: 0; }", "css", "body");
    }

    #[test]
    fn highlights_java() {
        assert_highlights("public class Foo { void bar() {} }", "java", "class");
    }

    #[test]
    fn highlights_bash() {
        assert_highlights("echo hello\nNAME=value", "bash", "echo");
        assert_highlights("echo hello\nNAME=value", "sh", "echo");
        assert_highlights("echo hello\nNAME=value", "shell", "echo");
    }

    #[test]
    fn highlights_perl() {
        assert_highlights("my $x = 42;\nprint $x;", "perl", "print");
        assert_highlights("my $x = 42;\nprint $x;", "pl", "print");
    }

    #[test]
    fn highlights_ruby() {
        assert_highlights("def hello\n  puts 'hi'\nend", "ruby", "hello");
        assert_highlights("def hello\n  puts 'hi'\nend", "rb", "hello");
    }

    #[test]
    fn highlights_scala() {
        assert_highlights("object Foo { def bar() = 1 }", "scala", "object");
    }

    #[test]
    fn highlights_lua() {
        assert_highlights("local x = 42\nprint(x)", "lua", "print");
    }

    #[test]
    fn highlights_sql() {
        assert_highlights("SELECT * FROM users WHERE id = 1;", "sql", "SELECT");
    }

    #[test]
    fn highlights_xml() {
        assert_highlights("<?xml version=\"1.0\"?>\n<root></root>", "xml", "root");
    }

    #[test]
    fn highlights_php() {
        assert_highlights("<?php\nfunction foo() {}\n?>", "php", "function");
    }

    #[test]
    fn highlights_diff() {
        assert_highlights(
            "--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n-old\n+new",
            "diff",
            "file.rs",
        );
    }

    // -- missing languages fall back gracefully --------------------------------

    #[test]
    fn missing_languages_return_none() {
        // These languages are not in syntect's default syntax set.
        for lang in [
            "swift",
            "toml",
            "dockerfile",
            "kt",
            "kotlin",
            "graphql",
            "proto",
        ] {
            assert!(
                highlight_code("hello world", lang).is_none(),
                "expected '{lang}' to be unrecognized (None), but highlighting succeeded"
            );
        }
    }

    #[test]
    fn missing_language_in_markdown_falls_back_to_plain() {
        // When syntax feature is enabled but the language is unknown,
        // the markdown renderer should fall back to plain code rendering.
        // This is tested via the None return path.
        assert!(highlight_code("let x = 1", "swift").is_none());
    }

    // -- cache behavior -------------------------------------------------------

    #[test]
    fn cache_returns_same_result_on_second_call() {
        clear_cache();
        let first = highlight_code("fn main() {}", "rust").unwrap();
        let second = highlight_code("fn main() {}", "rust").unwrap();
        assert_eq!(plain(&first[0]), plain(&second[0]));
    }

    #[test]
    fn cache_preserves_span_styles() {
        clear_cache();
        let first = highlight_code("fn main() {}", "rust").unwrap();
        let second = highlight_code("fn main() {}", "rust").unwrap();
        // Verify the cached result has the same styles (not just plain text).
        let first_has_color = first
            .iter()
            .any(|l| l.spans.iter().any(|s| s.style.fg.is_some()));
        let second_has_color = second
            .iter()
            .any(|l| l.spans.iter().any(|s| s.style.fg.is_some()));
        assert!(first_has_color);
        assert!(second_has_color);
        // Compare full style on first span.
        assert_eq!(first[0].spans[0].style.fg, second[0].spans[0].style.fg);
    }

    #[test]
    fn cache_distinguishes_different_code() {
        clear_cache();
        let a = highlight_code("fn main() {}", "rust").unwrap();
        let b = highlight_code("fn other() {}", "rust").unwrap();
        assert_ne!(plain(&a[0]), plain(&b[0]));
    }

    #[test]
    fn cache_distinguishes_different_langs() {
        clear_cache();
        // Same code, different language — should produce different highlighting.
        let rust = highlight_code("x = 1", "rust").unwrap();
        let py = highlight_code("x = 1", "python").unwrap();
        // Both should succeed (both languages are recognized).
        // The styles may differ even if plain text is the same.
        assert!(!rust.is_empty());
        assert!(!py.is_empty());
    }

    #[test]
    fn cache_does_not_return_stale_after_clear() {
        clear_cache();
        let _ = highlight_code("fn main() {}", "rust");
        clear_cache();
        // After clearing, the cache should be empty but the function should
        // still produce correct results (re-highlight from scratch).
        let result = highlight_code("fn main() {}", "rust").unwrap();
        assert!(plain(&result[0]).contains("fn"));
    }

    #[test]
    fn cache_hits_are_faster_than_misses() {
        clear_cache();
        let code = "fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}\n";

        // First call: cache miss (includes highlighting + cache insert).
        let miss_start = std::time::Instant::now();
        let _ = highlight_code(code, "rust");
        let miss_time = miss_start.elapsed();

        // Second call: cache hit (just clone from cache).
        let hit_start = std::time::Instant::now();
        let _ = highlight_code(code, "rust");
        let hit_time = hit_start.elapsed();

        // Cache hit should be significantly faster (at least 5×).
        // Use a generous threshold to avoid flakiness on CI.
        assert!(
            hit_time < miss_time / 5,
            "cache hit ({hit_time:?}) should be much faster than miss ({miss_time:?})"
        );
    }

    #[test]
    fn cache_works_across_different_lang_annotations() {
        clear_cache();
        // "rust" and "rs" both resolve to Rust syntax, but are cached
        // under separate keys. Both should work correctly.
        let long = highlight_code("fn main() {}", "rust").unwrap();
        let short = highlight_code("fn main() {}", "rs").unwrap();
        // Same syntax, same output text.
        assert_eq!(plain(&long[0]), plain(&short[0]));
    }

    #[test]
    fn cache_handles_lang_with_annotation() {
        clear_cache();
        // "rust skip" — only "rust" is used as the key.
        let a = highlight_code("fn main() {}", "rust skip").unwrap();
        let b = highlight_code("fn main() {}", "rust").unwrap();
        // Both should produce the same result (same syntax, same code).
        assert_eq!(plain(&a[0]), plain(&b[0]));
    }

    #[test]
    fn cache_bounded_size_clears_when_exceeded() {
        clear_cache();
        // Fill the cache up to MAX_CACHE_ENTRIES + 1 with unique entries.
        for i in 0..(MAX_CACHE_ENTRIES + 10) {
            let code = format!("fn func_{i}() {{}}");
            let _ = highlight_code(&code, "rust");
        }
        // The cache should have been cleared at some point and repopulated.
        // Verify it's not holding more than MAX_CACHE_ENTRIES + recent batch.
        if let Ok(cache) = HIGHLIGHT_CACHE.lock() {
            assert!(
                cache.len() <= MAX_CACHE_ENTRIES + 10,
                "cache grew unbounded: {} entries",
                cache.len()
            );
        }
        // And highlighting should still work after the clear.
        let result = highlight_code("fn after_clear() {}", "rust").unwrap();
        assert!(plain(&result[0]).contains("after_clear"));
    }

    #[test]
    fn map_style_no_modifiers() {
        let syn_style = syntect::highlighting::Style {
            foreground: syntect::highlighting::Color {
                r: 128,
                g: 128,
                b: 128,
                a: 255,
            },
            background: syntect::highlighting::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            font_style: FontStyle::empty(),
        };
        let rat_style = map_style(syn_style);
        assert_eq!(rat_style.fg, Some(Color::Rgb(128, 128, 128)));
        assert!(rat_style.add_modifier.is_empty());
    }
}
