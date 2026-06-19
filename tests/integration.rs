use lessmd::cli::RenderMode;
use lessmd::document::Document;
use lessmd::render::markdown::render_markdown;
use lessmd::render::text::render_text;
use lessmd::source::{ResolvedMode, read};

fn plain(line: &ratatui::text::Line) -> String {
    let mut s = String::new();
    for span in &line.spans {
        s.push_str(&span.content);
    }
    s
}

fn all_text(lines: &[ratatui::text::Line]) -> String {
    lines.iter().map(plain).collect::<Vec<_>>().join("\n")
}

// --- plain text (M1) -------------------------------------------------------

#[test]
fn plain_text_renders_line_count_and_content() {
    let lines = render_text("line one\nline two\nline three\n", 80, false);
    assert_eq!(lines.len(), 3);
    assert_eq!(plain(&lines[0]), "line one");
    assert_eq!(plain(&lines[2]), "line three");
}

#[test]
fn plain_fixture_renders() {
    let path = std::path::Path::new("tests/fixtures/plain.txt");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    assert_eq!(input.render_mode, ResolvedMode::Text { ansi: true });
    let doc = Document::new(&input, 200);
    assert_eq!(doc.line_count(), 6);
    assert_eq!(plain(&doc.lines[0]), "The quick brown fox");
    assert_eq!(plain(&doc.lines[5]), "Finally, the sixth line.");
}

#[test]
fn plain_fixture_wraps_when_narrow() {
    let path = std::path::Path::new("tests/fixtures/plain.txt");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 10);
    assert!(doc.line_count() > 6);
    assert_eq!(plain(&doc.lines[0]), "The quick ");
}

#[test]
fn plain_flag_strips_ansi_and_forces_text() {
    let path = std::path::Path::new("tests/fixtures/plain.txt");
    let input = read(Some(path), RenderMode::Plain).unwrap();
    assert_eq!(input.render_mode, ResolvedMode::Text { ansi: false });
}

// --- markdown (M2) ---------------------------------------------------------

#[test]
fn markdown_fixture_headings() {
    let path = std::path::Path::new("tests/fixtures/headings.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    assert_eq!(input.render_mode, ResolvedMode::Markdown);
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    for expected in ["Main Title", "Second Heading", "Sixth Heading"] {
        assert!(text.contains(expected), "missing heading: {expected}");
    }
}

#[test]
fn markdown_fixture_inline() {
    let path = std::path::Path::new("tests/fixtures/inline.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("bold"));
    assert!(text.contains("italic"));
    assert!(text.contains("code"));
    assert!(text.contains("strike"));
    assert!(text.contains("link"));
    assert!(text.contains("https://example.com"));
}

#[test]
fn markdown_fixture_lists() {
    let path = std::path::Path::new("tests/fixtures/lists.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("top level a"));
    assert!(text.contains("nested b"));
    assert!(text.contains("first"));
    assert!(text.contains("second"));
    assert!(text.contains("☑"));
    assert!(text.contains("☐"));
}

#[test]
fn markdown_fixture_blockquote() {
    let path = std::path::Path::new("tests/fixtures/blockquote.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("│"));
    assert!(text.contains("This is a quote."));
}

#[test]
fn markdown_fixture_table() {
    let path = std::path::Path::new("tests/fixtures/tables.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("name"));
    assert!(text.contains("age"));
    assert!(text.contains("alice"));
    assert!(text.contains("bob"));
}

#[test]
fn markdown_fixture_codeblocks() {
    let path = std::path::Path::new("tests/fixtures/codeblocks.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("rust"));
    assert!(text.contains("fn main()"));
    assert!(text.contains("let x = 42"));
}

#[test]
fn markdown_renders_via_render_module() {
    // The render dispatcher must route markdown mode to the markdown renderer.
    let md = "# Hello\n\nSome **bold** text.";
    let lines = render_markdown(md, 80);
    let text = all_text(&lines);
    assert!(text.contains("Hello"));
    assert!(text.contains("bold"));
}

#[cfg(not(feature = "mermaid"))]
#[test]
fn markdown_mermaid_fixture_falls_back_without_feature() {
    let path = std::path::Path::new("tests/fixtures/mermaid.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    assert_eq!(input.render_mode, ResolvedMode::Markdown);
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("Mermaid Fixture"));
    assert!(text.contains("┌─ mermaid"));
    assert!(text.contains("graph LR"));
    assert!(text.contains("mermaid render failed:"));
}

#[cfg(feature = "mermaid")]
#[test]
fn markdown_mermaid_fixture_renders_with_feature() {
    let path = std::path::Path::new("tests/fixtures/mermaid.md");
    let input = read(Some(path), RenderMode::Auto).unwrap();
    let doc = Document::new(&input, 200);
    let text = all_text(&doc.lines);
    assert!(text.contains("Mermaid Fixture"));
    assert!(text.contains("Start") || text.contains("A"));
    assert!(text.contains("Alice") || text.contains("Bob"));
    assert!(text.contains("this is invalid mermaid"));
    assert!(text.contains("mermaid render failed:"));
}
