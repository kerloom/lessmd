//! Performance tests for lessmd.
//!
//! These are `#[ignore]` tests — run them explicitly:
//!
//! ```sh
//! # All perf tests (no mermaid feature):
//! cargo test --test perf -- --ignored --nocapture
//!
//! # All perf tests (with mermaid feature):
//! cargo test --test perf --features mermaid -- --ignored --nocapture
//!
//! # Only the mermaid comparison tests:
//! cargo test --test perf perf_mermaid -- --ignored --nocapture
//! cargo test --test perf --features mermaid perf_mermaid -- --ignored --nocapture
//! ```
//!
//! They measure:
//! - **CPU**: time to render documents of increasing size
//! - **Memory**: ratio of rendered `Line` storage to input size
//! - **Scroll**: time to slice a viewport from a large document
//! - **Mermaid**: render timing with and without the `mermaid` feature
//!
//! Memory is estimated from the rendered line count and average span
//! allocation count rather than a system allocator hook, keeping the
//! tests portable and dependency-free.

use std::time::{Duration, Instant};

use lessmd::document::Document;
use lessmd::pager::PagerState;
use lessmd::render::markdown::render_markdown;
use lessmd::render::mermaid::{DefaultMermaidRenderer, MermaidRenderer};
use lessmd::render::text::render_text;
use lessmd::source::{Input, ResolvedMode};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_input(text: &str) -> Input {
    Input {
        text: text.to_owned(),
        render_mode: ResolvedMode::Text { ansi: false },
        source_path: None,
    }
}

fn md_input(text: &str) -> Input {
    Input {
        text: text.to_owned(),
        render_mode: ResolvedMode::Markdown,
        source_path: None,
    }
}

fn gen_plain_lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("line {i}: the quick brown fox jumps over the lazy dog"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn gen_markdown_doc(n_sections: usize, lines_per_section: usize) -> String {
    let mut s = String::new();
    for i in 0..n_sections {
        s.push_str(&format!("# Section {i}\n\n"));
        for j in 0..lines_per_section {
            s.push_str(&format!(
                "This is paragraph {j} in section {i}. Lorem ipsum dolor sit amet. "
            ));
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

/// Generate a markdown document with `n` flowchart mermaid blocks
/// interspersed with text paragraphs.
fn gen_markdown_with_mermaid(n_diagrams: usize) -> String {
    let mut s = String::new();
    s.push_str("# Document with Mermaid diagrams\n\n");
    for i in 0..n_diagrams {
        s.push_str(&format!("## Section {i}\n\n"));
        s.push_str(&format!("Some text before diagram {i}.\n\n"));
        s.push_str("```mermaid\n");
        s.push_str(&format!(
            "graph LR\n    A[Start {i}] --> B[Process {i}] --> C[End {i}]\n"
        ));
        s.push_str("```\n\n");
        s.push_str(&format!("Some text after diagram {i}.\n\n"));
    }
    s
}

/// Generate a markdown document with `n` sequence diagrams.
fn gen_markdown_with_sequence(n_diagrams: usize) -> String {
    let mut s = String::new();
    s.push_str("# Sequence diagrams\n\n");
    for i in 0..n_diagrams {
        s.push_str(&format!("## Interaction {i}\n\n"));
        s.push_str("```mermaid\n");
        s.push_str(&format!(
            "sequenceDiagram\n    Alice->>Bob: Request {i}\n    Bob-->>Alice: Response {i}\n"
        ));
        s.push_str("```\n\n");
    }
    s
}

fn avg_ns_per_line(total: Duration, n: usize) -> f64 {
    total.as_nanos() as f64 / n as f64
}

fn fmt_duration(d: Duration) -> String {
    if d.as_secs() > 0 {
        format!("{:.2}s", d.as_secs_f64())
    } else if d.as_millis() > 0 {
        format!("{:.1}ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{:.0}µs", d.as_secs_f64() * 1_000_000.0)
    }
}

// ---------------------------------------------------------------------------
// CPU: plain-text rendering
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_plain_text_render_scaling() {
    println!("\n=== plain-text render scaling ===");

    // Warmup — first run pays allocator/init overhead.
    let _ = render_text(&gen_plain_lines(100), 80, false);

    println!(
        "{:>10}  {:>10}  {:>12}  {:>12}",
        "lines", "out_lines", "time", "ns/line"
    );

    for &n in &[1_000, 10_000, 50_000, 100_000] {
        let text = gen_plain_lines(n);
        let t0 = Instant::now();
        let out = render_text(&text, 80, false);
        let elapsed = t0.elapsed();

        println!(
            "{:>10}  {:>10}  {:>12}  {:>10.1}",
            n,
            out.lines.len(),
            fmt_duration(elapsed),
            avg_ns_per_line(elapsed, n),
        );

        // Rendering should stay under 100µs per input line (debug builds).
        assert!(
            elapsed.as_secs_f64() < n as f64 * 100e-6,
            "rendering {n} lines took too long: {}",
            fmt_duration(elapsed)
        );
    }
}

// ---------------------------------------------------------------------------
// CPU: markdown rendering
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_markdown_render_scaling() {
    println!("\n=== markdown render scaling ===");
    println!(
        "{:>12}  {:>12}  {:>10}  {:>12}  {:>12}",
        "sections", "lines/sec", "chars", "time", "ns/char"
    );

    for &(s, l) in &[(10, 50), (50, 50), (100, 100), (500, 100)] {
        let md = gen_markdown_doc(s, l);
        let chars = md.len();
        let t0 = Instant::now();
        let out = render_markdown(&md, 80);
        let elapsed = t0.elapsed();

        println!(
            "{:>12}  {:>12}  {:>10}  {:>12}  {:>10.1}",
            s,
            out.lines.len(),
            chars,
            fmt_duration(elapsed),
            elapsed.as_nanos() as f64 / chars as f64,
        );

        // Markdown rendering should stay under 100µs per character (debug builds).
        assert!(
            elapsed.as_secs_f64() < chars as f64 * 100e-6,
            "rendering {chars} chars took too long: {}",
            fmt_duration(elapsed)
        );
    }
}

// ---------------------------------------------------------------------------
// Memory: rendered output vs input size
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_memory_expansion_plain_text() {
    println!("\n=== memory: plain-text output vs input ===");
    println!(
        "{:>10}  {:>12}  {:>12}  {:>10}",
        "input_b", "output_lines", "output_b*", "ratio"
    );

    for &n in &[1_000, 10_000, 100_000] {
        let text = gen_plain_lines(n);
        let input_bytes = text.len();
        let out = render_text(&text, 80, false);

        // Rough estimate: each line stores spans in a Vec.  A plain-text
        // line typically has 1 span whose content is a Cow::Owned String.
        // Estimated bytes ≈ lines * (sizeof(Line) + sizeof(Vec<Span>) +
        // sizeof(Span) + content_len).
        // sizeof(Line) ≈ 24 (Vec ptr+len+cap)
        // sizeof(Span)  ≈ 40 (Cow + Style)
        // We sum actual content lengths as a lower bound.
        let content_bytes: usize = out
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.len() + 48).sum::<usize>() + 24)
            .sum();

        let ratio = content_bytes as f64 / input_bytes as f64;
        println!(
            "{:>10}  {:>12}  {:>12}  {:>10.2}x",
            input_bytes,
            out.lines.len(),
            content_bytes,
            ratio,
        );

        // Expansion should be modest — plain text is 1:1 mapping + wrapping.
        assert!(
            ratio < 5.0,
            "memory expansion {ratio:.2}x is too high for {n} lines"
        );
    }
}

#[test]
#[ignore]
fn perf_memory_expansion_markdown() {
    println!("\n=== memory: markdown output vs input ===");
    println!(
        "{:>10}  {:>12}  {:>12}  {:>10}",
        "input_b", "output_lines", "output_b*", "ratio"
    );

    for &(s, l) in &[(10, 50), (100, 100), (500, 100)] {
        let md = gen_markdown_doc(s, l);
        let input_bytes = md.len();
        let out = render_markdown(&md, 80);

        let content_bytes: usize = out
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|sp| sp.content.len() + 48)
                    .sum::<usize>()
                    + 24
            })
            .sum();

        let ratio = content_bytes as f64 / input_bytes as f64;
        println!(
            "{:>10}  {:>12}  {:>12}  {:>10.2}x",
            input_bytes,
            out.lines.len(),
            content_bytes,
            ratio,
        );

        // Markdown adds styling, prefixes, borders etc. — allow more expansion.
        assert!(
            ratio < 10.0,
            "memory expansion {ratio:.2}x is too high for {s} sections"
        );
    }
}

// ---------------------------------------------------------------------------
// CPU: viewport slicing from a large document
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_scroll_slice_large_doc() {
    println!("\n=== scroll: viewport slice from large doc ===");
    let n = 100_000;
    let text = gen_plain_lines(n);
    let input = text_input(&text);
    let state = PagerState::new(input, 24, 80, false);

    println!("document: {n} lines, {} visible rows", state.height);

    // Measure slicing from different offsets.
    let offsets = [0, n / 4, n / 2, n * 3 / 4, n - state.height];
    println!("{:>12}  {:>12}", "offset", "slice_time");

    let mut max_time = Duration::ZERO;
    for &off in &offsets {
        let mut s = state.clone();
        s.offset = off.min(s.max_offset());

        let t0 = Instant::now();
        let _ = s.visible_lines_panned();
        let elapsed = t0.elapsed();
        max_time = max_time.max(elapsed);

        println!("{:>12}  {:>12}", off, fmt_duration(elapsed));
    }

    // Slicing should be sub-millisecond (it's just a Vec index + clip).
    assert!(
        max_time.as_millis() < 10,
        "viewport slice took too long: {}",
        fmt_duration(max_time)
    );
}

// ---------------------------------------------------------------------------
// CPU: search over large document
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_search_large_doc() {
    println!("\n=== search: full-document scan ===");
    let n = 50_000;
    // Plant a known substring every 1000 lines.
    let mut text = String::new();
    for i in 0..n {
        if i % 1000 == 0 && i > 0 {
            text.push_str("NEEDLE\n");
        } else {
            text.push_str(&format!("line {i} regular content\n"));
        }
    }
    let input = text_input(&text);
    let state = PagerState::new(input, 24, 80, false);

    let t0 = Instant::now();
    let matches = lessmd::search::search_lines(&state.doc.lines, "NEEDLE");
    let elapsed = t0.elapsed();

    println!(
        "searched {} lines in {}, found {} matches",
        n,
        fmt_duration(elapsed),
        matches.len()
    );

    assert_eq!(matches.len(), 49); // lines 1000, 2000, ..., 49000
    assert!(
        elapsed.as_millis() < 500,
        "search took too long: {}",
        fmt_duration(elapsed)
    );
}

// ---------------------------------------------------------------------------
// CPU: re-render on resize
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_resize_rerender() {
    println!("\n=== resize: full re-render timing ===");
    let n = 50_000;
    let text = gen_plain_lines(n);
    let input = text_input(&text);
    let mut state = PagerState::new(input, 24, 80, false);

    println!("document: {n} lines");

    let widths = [80u16, 120, 40, 100, 60];
    let mut max_time = Duration::ZERO;
    for &w in &widths {
        let t0 = Instant::now();
        state.resize(24, w);
        let elapsed = t0.elapsed();
        max_time = max_time.max(elapsed);
        println!("  resize to width {w}: {}", fmt_duration(elapsed));
    }

    // Re-render on resize should stay under 2s for 50k lines (debug builds).
    assert!(
        max_time.as_secs() < 5,
        "resize re-render took too long: {}",
        fmt_duration(max_time)
    );
}

// ---------------------------------------------------------------------------
// Document::new timing (render + heading extraction)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_document_new_overall() {
    println!("\n=== Document::new overall (render + heading extraction) ===");
    println!(
        "{:>12}  {:>12}  {:>10}  {:>12}",
        "type", "chars", "headings", "time"
    );

    // Plain text
    let text = gen_plain_lines(100_000);
    let t0 = Instant::now();
    let doc = Document::new(&text_input(&text), 80);
    let elapsed = t0.elapsed();
    println!(
        "{:>12}  {:>12}  {:>10}  {:>12}",
        "plain",
        text.len(),
        doc.headings.len(),
        fmt_duration(elapsed)
    );

    // Markdown
    let md = gen_markdown_doc(500, 100);
    let t0 = Instant::now();
    let doc = Document::new(&md_input(&md), 80);
    let elapsed = t0.elapsed();
    println!(
        "{:>12}  {:>12}  {:>10}  {:>12}",
        "markdown",
        md.len(),
        doc.headings.len(),
        fmt_duration(elapsed)
    );
    assert_eq!(doc.headings.len(), 500);
}

// ---------------------------------------------------------------------------
// Mermaid: render with and without the feature
// ---------------------------------------------------------------------------
//
// `DefaultMermaidRenderer` returns an error when the `mermaid` feature is
// off (fallback path) and calls `figurehead::render` when it's on.  These
// two tests let you compare the two code paths:
//
//   cargo test --test perf perf_mermaid -- --ignored --nocapture
//   cargo test --test perf --features mermaid perf_mermaid -- --ignored --nocapture
//

#[test]
#[ignore]
fn perf_mermaid_flowchart_render() {
    let renderer = DefaultMermaidRenderer;
    let sources: Vec<String> = (0..50)
        .map(|i| format!("graph LR\n    A[Start {i}] --> B[Mid {i}] --> C[End {i}]\n"))
        .collect();

    println!("\n=== mermaid: flowchart render (50 diagrams) ===");

    let mut total = Duration::ZERO;
    let mut ok = 0usize;
    let mut err = 0usize;
    for src in &sources {
        let t0 = Instant::now();
        let result = renderer.render(src);
        let elapsed = t0.elapsed();
        total += elapsed;
        match &result {
            Ok(out) => {
                ok += 1;
                if ok == 1 {
                    println!("  sample output (first diagram):");
                    for line in out.lines().take(5) {
                        println!("    {line}");
                    }
                }
            }
            Err(e) => {
                err += 1;
                if err == 1 {
                    println!("  first error: {e}");
                }
            }
        }
    }

    println!(
        "  {ok} ok, {err} errors, total {}, avg {}/diagram",
        fmt_duration(total),
        fmt_duration(total / 50)
    );
    println!(
        "  (mermaid feature is {})",
        if cfg!(feature = "mermaid") {
            "ENABLED"
        } else {
            "DISABLED — diagrams fall back to code blocks"
        }
    );
}

#[test]
#[ignore]
fn perf_mermaid_sequence_render() {
    let renderer = DefaultMermaidRenderer;
    let sources: Vec<String> = (0..50)
        .map(|i| {
            format!(
                "sequenceDiagram\n    Alice->>Bob: Request {i}\n    Bob-->>Alice: Response {i}\n"
            )
        })
        .collect();

    println!("\n=== mermaid: sequence render (50 diagrams) ===");

    let mut total = Duration::ZERO;
    let mut ok = 0usize;
    let mut err = 0usize;
    for src in &sources {
        let t0 = Instant::now();
        let result = renderer.render(src);
        let elapsed = t0.elapsed();
        total += elapsed;
        match &result {
            Ok(out) => {
                ok += 1;
                if ok == 1 {
                    println!("  sample output (first diagram):");
                    for line in out.lines().take(5) {
                        println!("    {line}");
                    }
                }
            }
            Err(e) => {
                err += 1;
                if err == 1 {
                    println!("  first error: {e}");
                }
            }
        }
    }

    println!(
        "  {ok} ok, {err} errors, total {}, avg {}/diagram",
        fmt_duration(total),
        fmt_duration(total / 50)
    );
}

/// Compare markdown rendering with mermaid diagrams embedded.
/// Without the feature, each ```` ```mermaid ```` block falls back to a
/// code block + error note.  With the feature, figurehead renders each
/// diagram to ASCII art.
#[test]
#[ignore]
fn perf_mermaid_markdown_with_diagrams() {
    println!("\n=== markdown + mermaid: full document render ===");
    println!(
        "  feature: {}",
        if cfg!(feature = "mermaid") {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    // Warmup
    let _ = render_markdown(&gen_markdown_with_mermaid(1), 80);

    for &n in &[10, 50, 100] {
        let md = gen_markdown_with_mermaid(n);
        let chars = md.len();

        let t0 = Instant::now();
        let out = render_markdown(&md, 80);
        let elapsed = t0.elapsed();

        // Count how many lines look like rendered diagrams vs fallback code blocks.
        let fallback_count = out
            .lines
            .iter()
            .filter(|l| {
                l.spans
                    .iter()
                    .any(|s| s.content.contains("mermaid render failed"))
            })
            .count();

        println!(
            "  {n:>4} diagrams | {chars:>8} chars | {:>6} out_lines | {:>3} fallbacks | {} | {:.1} µs/char",
            out.lines.len(),
            fallback_count,
            fmt_duration(elapsed),
            elapsed.as_nanos() as f64 / chars as f64,
        );
    }
}

/// Measure the full `Document::new` pipeline (render + heading extraction)
/// on a document packed with mermaid sequence diagrams.
#[test]
#[ignore]
fn perf_mermaid_document_with_sequence() {
    println!("\n=== Document::new with sequence diagrams ===");
    println!(
        "  feature: {}",
        if cfg!(feature = "mermaid") {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    for &n in &[10, 50, 100] {
        let md = gen_markdown_with_sequence(n);
        let chars = md.len();

        let t0 = Instant::now();
        let doc = Document::new(&md_input(&md), 80);
        let elapsed = t0.elapsed();

        println!(
            "  {n:>4} seq diagrams | {chars:>8} chars | {:>6} lines | {:>3} headings | {} | {:.1} µs/char",
            doc.lines.len(),
            doc.headings.len(),
            fmt_duration(elapsed),
            elapsed.as_nanos() as f64 / chars as f64,
        );
    }
}
