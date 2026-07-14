use std::time::{Duration, Instant};

use lessmd::render::markdown::render_markdown;
#[cfg(feature = "mermaid")]
use lessmd::render::mermaid::clear_cache;

#[test]
fn worldlink_diagrams_do_not_block_document_rendering() {
    let source = include_str!("fixtures/worldlink_diagrams.md");
    #[cfg(feature = "mermaid")]
    clear_cache();

    let started = Instant::now();
    let output = render_markdown(source, 120);

    assert!(!output.lines.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "render took {:?}, likely hung on a mermaid diagram",
        started.elapsed()
    );

    #[cfg(feature = "mermaid")]
    assert_eq!(
        output.mermaid_failures, 1,
        "expected only the cyclic state diagram to fail, got {}",
        output.mermaid_failures
    );

    #[cfg(not(feature = "mermaid"))]
    assert_eq!(output.mermaid_failures, 0);
}
