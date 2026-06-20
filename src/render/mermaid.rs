//! Mermaid rendering adapter.
//!
//! The markdown renderer depends on the small trait in this module, keeping the
//! concrete `figurehead` dependency optional and easy to replace in tests.

#[cfg(feature = "mermaid")]
use std::collections::HashMap;
#[cfg(feature = "mermaid")]
use std::sync::{LazyLock, Mutex};

/// Converts Mermaid source into terminal-friendly text.
pub trait MermaidRenderer {
    fn render(&self, source: &str) -> Result<String, String>;
}

/// Default renderer used by markdown rendering.
pub struct DefaultMermaidRenderer;

#[cfg(feature = "mermaid")]
impl MermaidRenderer for DefaultMermaidRenderer {
    fn render(&self, source: &str) -> Result<String, String> {
        if let Some(cached) = cache_get(source) {
            return cached;
        }

        let result = match render_with_figurehead(source) {
            Ok(rendered) => Ok(rendered),
            Err(first_err) => match sanitize_sequence(source) {
                Some(sanitized) => render_with_figurehead(&sanitized).map_err(|_| first_err),
                None => Err(first_err),
            },
        };
        cache_insert(source.to_owned(), result.clone());
        result
    }
}

#[cfg(feature = "mermaid")]
const MAX_CACHE_ENTRIES: usize = 128;

#[cfg(feature = "mermaid")]
type MermaidCache = HashMap<String, Result<String, String>>;

#[cfg(feature = "mermaid")]
static MERMAID_CACHE: LazyLock<Mutex<MermaidCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "mermaid")]
fn cache_get(source: &str) -> Option<Result<String, String>> {
    MERMAID_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(source).cloned())
}

#[cfg(feature = "mermaid")]
fn cache_insert(source: String, result: Result<String, String>) {
    if let Ok(mut cache) = MERMAID_CACHE.lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(source, result);
    }
}

/// Clear the Mermaid render cache. Useful for tests and document switches.
#[cfg(feature = "mermaid")]
pub fn clear_cache() {
    if let Ok(mut cache) = MERMAID_CACHE.lock() {
        cache.clear();
    }
}

#[cfg(feature = "mermaid")]
#[cfg(test)]
fn cache_len() -> usize {
    MERMAID_CACHE.lock().map(|cache| cache.len()).unwrap_or(0)
}

#[cfg(feature = "mermaid")]
fn render_with_figurehead(source: &str) -> Result<String, String> {
    catch_mermaid_panic(|| figurehead::render(source))?.map_err(|e| e.to_string())
}

#[cfg(feature = "mermaid")]
fn sanitize_sequence(source: &str) -> Option<String> {
    if !source.trim_start().starts_with("sequenceDiagram") {
        return None;
    }

    let mut changed = false;
    let mut out = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if is_self_message(trimmed) {
            changed = true;
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    changed.then_some(out)
}

#[cfg(feature = "mermaid")]
fn is_self_message(line: &str) -> bool {
    for arrow in ["-->>", "->>", "-->", "->", "--)", "-)"] {
        let Some(arrow_pos) = line.find(arrow) else {
            continue;
        };
        let from = line[..arrow_pos].trim();
        let rest = &line[arrow_pos + arrow.len()..];
        let Some(colon_pos) = rest.find(':') else {
            continue;
        };
        let to = rest[..colon_pos].trim();
        return !from.is_empty() && from == to;
    }
    false
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

    #[cfg(feature = "mermaid")]
    fn cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|err| err.into_inner())
    }

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
        let _guard = cache_test_lock();
        clear_cache();
        let renderer = DefaultMermaidRenderer;
        let output = renderer.render("graph LR\nA[Start] --> B[End]").unwrap();
        assert!(!output.trim().is_empty());
        assert!(output.contains("Start") || output.contains("A"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_renders_sequence_diagram() {
        let _guard = cache_test_lock();
        clear_cache();
        let renderer = DefaultMermaidRenderer;
        let output = renderer
            .render("sequenceDiagram\nAlice->>Bob: Hello")
            .unwrap();
        assert!(!output.trim().is_empty());
        assert!(output.contains("Alice") || output.contains("Bob") || output.contains('─'));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn default_renderer_caches_mermaid_results() {
        let _guard = cache_test_lock();
        clear_cache();
        let renderer = DefaultMermaidRenderer;
        let source = "graph LR\nA[Start] --> B[End]";

        let first = renderer.render(source).unwrap();
        assert_eq!(cache_get(source).unwrap().unwrap(), first);
        let second = renderer.render(source).unwrap();

        assert_eq!(second, first);
        assert_eq!(cache_get(source).unwrap().unwrap(), first);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn mermaid_cache_clears_when_entry_limit_is_exceeded() {
        let _guard = cache_test_lock();
        clear_cache();
        for i in 0..MAX_CACHE_ENTRIES {
            cache_insert(format!("source {i}"), Ok(format!("rendered {i}")));
        }
        assert_eq!(cache_len(), MAX_CACHE_ENTRIES);

        cache_insert("overflow".to_owned(), Ok("rendered overflow".to_owned()));

        assert_eq!(cache_get("overflow").unwrap().unwrap(), "rendered overflow");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_panics_are_returned_as_errors() {
        let err = catch_mermaid_panic(|| std::panic::panic_any("boom")).unwrap_err();
        assert_eq!(err, "mermaid renderer panicked: boom");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn sequence_self_messages_are_removed_before_retry() {
        let source = "sequenceDiagram\n    A->>A: wait\n    A->>B: done";
        let sanitized = sanitize_sequence(source).unwrap();
        assert!(!sanitized.contains("A->>A"));
        assert!(sanitized.contains("A->>B"));
    }
}
