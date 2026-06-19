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
        catch_mermaid_panic(|| figurehead::render(source))?.map_err(|e| e.to_string())
    }
}

#[cfg(feature = "mermaid")]
static PANIC_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(feature = "mermaid")]
fn catch_mermaid_panic<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    let _guard = PANIC_HOOK_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(f).map_err(|payload| panic_message(payload.as_ref()));
    std::panic::set_hook(hook);
    result
}

#[cfg(feature = "mermaid")]
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        format!("mermaid renderer panicked: {msg}")
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        format!("mermaid renderer panicked: {msg}")
    } else {
        "mermaid renderer panicked".to_owned()
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

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_panics_are_returned_as_errors() {
        let err = catch_mermaid_panic(|| std::panic::panic_any("boom")).unwrap_err();
        assert_eq!(err, "mermaid renderer panicked: boom");
    }
}
