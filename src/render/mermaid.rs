//! Mermaid rendering adapter.
//!
//! The markdown renderer depends on the small trait in this module, keeping the
//! concrete `figurehead` dependency optional and easy to replace in tests.

/// Converts Mermaid source into terminal-friendly text.
pub trait MermaidRenderer {
    fn render(&self, source: &str) -> Result<String, String>;
}

/// Default renderer used by markdown rendering.
pub struct DefaultMermaidRenderer;

#[cfg(feature = "mermaid")]
impl MermaidRenderer for DefaultMermaidRenderer {
    fn render(&self, source: &str) -> Result<String, String> {
        figurehead::render(source).map_err(|e| e.to_string())
    }
}

#[cfg(not(feature = "mermaid"))]
impl MermaidRenderer for DefaultMermaidRenderer {
    fn render(&self, _source: &str) -> Result<String, String> {
        Err("mermaid support is not enabled".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRenderer;

    impl MermaidRenderer for MockRenderer {
        fn render(&self, source: &str) -> Result<String, String> {
            Ok(format!("rendered: {source}"))
        }
    }

    #[test]
    fn mermaid_renderer_trait_is_swappable() {
        let renderer = MockRenderer;
        assert_eq!(
            renderer.render("graph LR; A-->B").unwrap(),
            "rendered: graph LR; A-->B"
        );
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_renders_simple_flowchart() {
        let renderer = DefaultMermaidRenderer;
        let output = renderer.render("graph LR\nA[Start] --> B[End]").unwrap();
        assert!(!output.trim().is_empty());
        assert!(output.contains("Start") || output.contains("A"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_renders_sequence_diagram() {
        let renderer = DefaultMermaidRenderer;
        let output = renderer
            .render("sequenceDiagram\nAlice->>Bob: Hello")
            .unwrap();
        assert!(!output.trim().is_empty());
        assert!(output.contains("Alice") || output.contains("Bob") || output.contains('─'));
    }
}
