//! Mermaid rendering adapter.
//!
//! The markdown renderer depends on the small trait in this module, keeping the
//! concrete `figurehead` dependency optional and easy to replace in tests.

#[cfg(feature = "mermaid")]
use std::collections::HashMap;
#[cfg(feature = "mermaid")]
use std::sync::{LazyLock, Mutex};
#[cfg(feature = "mermaid")]
use std::time::Duration;

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

        install_quiet_mermaid_hook();
        let cache_key = source.to_owned();
        let render_source = cache_key.clone();
        let result = run_with_timeout(RENDER_TIMEOUT, move || render_attempt(&render_source));
        cache_insert(cache_key, result.clone());
        result
    }
}

/// Deadline for each Figurehead render, including retries after a panic.
#[cfg(feature = "mermaid")]
const RENDER_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(feature = "mermaid")]
const MERMAID_THREAD: &str = "lessmd-mermaid";

/// Run `f` on a worker thread, abandoning it on timeout.
///
/// Rust cannot kill a thread stuck in a loop, so a hung render leaks its
/// worker thread until the process exits (the thread eventually finishes or
/// dies on `tx.send` failing into a dropped channel). The result is cached,
/// so a given pathological source leaks at most one thread per program run.
#[cfg(feature = "mermaid")]
fn run_with_timeout<F>(timeout: Duration, f: F) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    use std::sync::mpsc::{self, RecvTimeoutError};

    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(MERMAID_THREAD.to_owned())
        .spawn(move || {
            let _ = tx.send(f());
        });
    if let Err(err) = spawned {
        return Err(format!("could not spawn mermaid worker: {err}"));
    }

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            Err(format!("render timed out after {}ms", timeout.as_millis()))
        }
        Err(RecvTimeoutError::Disconnected) => {
            Err("mermaid worker stopped unexpectedly".to_owned())
        }
    }
}

#[cfg(feature = "mermaid")]
fn mermaid_body_starts_with(source: &str, prefix: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        return trimmed.starts_with(prefix);
    }
    false
}

/// Retry once with sequence self-messages stripped (figurehead panics on those).
#[cfg(feature = "mermaid")]
fn render_attempt(source: &str) -> Result<String, String> {
    let normalized = normalize_flowchart_syntax(source);
    match render_with_figurehead(normalized.as_deref().unwrap_or(source)) {
        Ok(rendered) => Ok(rendered),
        Err(first_err) => match sanitize_sequence(source) {
            Some(sanitized) => render_with_figurehead(&sanitized).map_err(|_| first_err),
            None => Err(first_err),
        },
    }
}

#[cfg(feature = "mermaid")]
fn normalize_flowchart_syntax(source: &str) -> Option<String> {
    if !mermaid_body_starts_with(source, "flowchart") && !mermaid_body_starts_with(source, "graph")
    {
        return None;
    }

    let mut changed = false;
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        let indentation_len = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let subgraph = trimmed
            .strip_prefix("subgraph ")
            .and_then(|declaration| declaration.split_once("[\""))
            .and_then(|(id, title)| {
                let title = title.strip_suffix("\"]")?;
                (!id.is_empty()
                    && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !title.contains('"'))
                .then(|| format!("subgraph \"{title}\""))
            });

        if let Some(normalized) = subgraph {
            output.push_str(&line[..indentation_len]);
            output.push_str(&normalized);
            changed = true;
        } else {
            let normalized = line
                .replace("[\"", "[")
                .replace("\"]", "]")
                .replace("{\"", "{")
                .replace("\"}", "}")
                .replace("<br/>", " ")
                .replace("<br />", " ")
                .replace("<br>", " ")
                .replace("<b>", "")
                .replace("</b>", "");
            changed |= normalized != line;
            output.push_str(&normalized);
        }
        output.push('\n');
    }

    changed.then_some(output)
}

/// Silence panics from mermaid worker threads (already caught by
/// [`catch_panic`]) so they don't scribble over the TUI; forward all others.
#[cfg(feature = "mermaid")]
fn install_quiet_mermaid_hook() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let on_mermaid_thread = std::thread::current()
                .name()
                .is_some_and(|name| name == MERMAID_THREAD);
            if !on_mermaid_thread {
                default_hook(info);
            }
        }));
    });
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
    catch_panic(|| figurehead::render(source))?.map_err(|e| e.to_string())
}

#[cfg(feature = "mermaid")]
fn sanitize_sequence(source: &str) -> Option<String> {
    if !mermaid_body_starts_with(source, "sequenceDiagram") {
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

/// Run `f`, converting a panic into an `Err`. Panic output is suppressed for
/// mermaid worker threads by [`install_quiet_mermaid_hook`].
#[cfg(feature = "mermaid")]
fn catch_panic<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(f).map_err(|payload| panic_message(payload.as_ref()))
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
    fn figurehead_renders_named_subgraphs_without_dropping_their_nodes() {
        let _guard = cache_test_lock();
        clear_cache();
        let source = r#"flowchart TB
    subgraph phase_one["Phase One"]
        A1["Request accepted"] --> A2["Validate input<br/><b>required field</b>"]
        A2 --> A3["Create record"]
    end

    subgraph phase_two["Phase Two"]
        B1["Response received"] --> B2{"Checks pass?"}
        B2 -->|no| B3["Stop processing"]
        B2 -->|yes| B4["Publish output"]
    end

    A3 --> FINAL["Completed"]
    B4 --> FINAL

    style A2 fill:#ffe6e6,stroke:#c00
    style B4 fill:#e6ffe9,stroke:#0a0"#;

        let output = DefaultMermaidRenderer
            .render(source)
            .expect("should render");

        for expected in [
            "Request accepted",
            "Validate input",
            "Create record",
            "Phase Two",
            "Response received",
            "Checks pass?",
            "Stop processing",
            "Publish output",
            "Completed",
        ] {
            assert!(output.contains(expected), "missing {expected:?}:\n{output}");
        }
        assert!(!output.contains("<br"));
        assert!(!output.contains("<b>"));
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
        // Keep the test log clean; the panic is still caught.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let err = catch_panic(|| std::panic::panic_any("boom")).unwrap_err();
        std::panic::set_hook(prev);
        assert_eq!(err, "mermaid renderer panicked: boom");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn figurehead_renders_cyclic_state_diagram_with_notes() {
        let _guard = cache_test_lock();
        clear_cache();
        let src = r#"stateDiagram-v2
    [*] --> Processing: instruction sent to Citi
    Processing --> Credited: success
    Processing --> ManualRetryRequired: technical failure (1524)
    ManualRetryRequired --> Processing: Retry
    ManualRetryRequired --> ManualRetryRequired: Retry fails again
    Credited --> [*]
    note right of ManualRetryRequired
        Preserve the original failure
        in the audit log
    end note"#;
        let started = std::time::Instant::now();
        let output = DefaultMermaidRenderer.render(src).expect("should render");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "state render took {:?}",
            started.elapsed()
        );
        for expected in [
            "Processing",
            "Credited",
            "ManualRetryRequired",
            "Retry fails again",
            "Preserve the original failure",
        ] {
            assert!(output.contains(expected), "missing {expected:?}:\n{output}");
        }
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn run_with_timeout_abandons_a_hanging_render() {
        let result = run_with_timeout(Duration::from_millis(50), || {
            loop {
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn run_with_timeout_passes_fast_results_through() {
        let ok = run_with_timeout(Duration::from_secs(5), || Ok("done".to_owned()));
        assert_eq!(ok.unwrap(), "done");
        let err = run_with_timeout(Duration::from_secs(5), || Err("nope".to_owned()));
        assert_eq!(err.unwrap_err(), "nope");
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
