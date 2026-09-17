//! Mermaid rendering adapter.
//!
//! The markdown renderer depends on the small trait in this module, keeping the
//! concrete `merman` dependency optional and easy to replace in tests.

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
        let control = merman::OperationControl::new().with_deadline(RENDER_TIMEOUT);
        let render_control = control.clone();
        let result = run_with_timeout(
            RENDER_TIMEOUT,
            move || control.cancel(),
            move || render_with_merman(&render_source, render_control),
        );
        cache_insert(cache_key, result.clone());
        result
    }
}

/// Deadline for each Merman render.
#[cfg(feature = "mermaid")]
const RENDER_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(feature = "mermaid")]
const MERMAID_THREAD: &str = "lessmd-mermaid";

/// Run `f` on a worker thread, invoking `on_timeout` before abandoning it.
///
/// Rust cannot kill a thread stuck outside cooperative checkpoints, so it may
/// remain alive until the work finishes or the process exits.
#[cfg(feature = "mermaid")]
fn run_with_timeout<F, C>(timeout: Duration, on_timeout: C, f: F) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
    C: FnOnce(),
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
            on_timeout();
            Err(format!("render timed out after {}ms", timeout.as_millis()))
        }
        Err(RecvTimeoutError::Disconnected) => {
            Err("mermaid worker stopped unexpectedly".to_owned())
        }
    }
}

/// Silence caught panics from Mermaid worker threads so they do not scribble
/// over the TUI; forward all other panics.
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
fn render_with_merman(source: &str, control: merman::OperationControl) -> Result<String, String> {
    use merman::ascii::AsciiRenderOptions;
    use merman::{AsciiRequest, ParseOptions, RenderOutput, RenderRequest, Renderer};

    static RENDERER: LazyLock<Renderer> =
        LazyLock::new(|| Renderer::new().with_parse_options(ParseOptions::strict()));

    // Merman's terminal backend otherwise emits these common label tags literally.
    let source = source.replace("<b>", "").replace("</b>", "");
    let output = catch_panic(|| {
        RENDERER.render(RenderRequest::ascii(
            &source,
            control,
            AsciiRequest {
                options: AsciiRenderOptions::unicode(),
                ..Default::default()
            },
        ))
    })?
    .map_err(|err| err.to_string())?;
    let RenderOutput::Ascii(Some(report)) = output else {
        return Err("no Mermaid diagram detected".to_owned());
    };
    Ok(report.text)
}

/// Run `f`, converting a panic into an `Err`. Worker panic output is suppressed
/// by [`install_quiet_mermaid_hook`].
#[cfg(feature = "mermaid")]
fn catch_panic<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|payload| panic_message(payload.as_ref()))
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
    fn merman_renders_simple_flowchart() {
        let _guard = cache_test_lock();
        clear_cache();
        let renderer = DefaultMermaidRenderer;
        let output = renderer.render("graph LR\nA[Start] --> B[End]").unwrap();
        assert!(!output.trim().is_empty());
        assert!(output.contains("Start") || output.contains("A"));
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_preserves_unicode_grapheme_labels() {
        let source = "flowchart LR\nA[\"Cafe\u{301} 👩‍💻 東京\"] --> B[Done]";
        let output = render_with_merman(source, merman::OperationControl::new()).unwrap();

        for expected in ["Cafe\u{301}", "👩‍💻", "東京", "Done"] {
            assert!(output.contains(expected), "missing {expected:?}:\n{output}");
        }
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_wraps_long_flowchart_labels_without_losing_words() {
        let label = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
        let source = format!("flowchart TB\nA[\"{label}\"] --> B[Done]");
        let output = render_with_merman(&source, merman::OperationControl::new()).unwrap();

        assert!(!output.contains(label), "label should wrap:\n{output}");
        for expected in label.split_whitespace() {
            assert!(output.contains(expected), "missing {expected:?}:\n{output}");
        }
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_reports_when_no_diagram_is_present() {
        let error = render_with_merman(
            "ordinary prose without a diagram",
            merman::OperationControl::new(),
        )
        .unwrap_err();

        assert_eq!(error, "No Mermaid diagram type detected");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_renders_named_subgraphs_without_dropping_their_nodes() {
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
    fn merman_renders_sequence_diagram() {
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
    fn renderer_panics_are_returned_as_errors() {
        // Keep the test log clean; the panic is still caught.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let err = catch_panic(|| std::panic::panic_any("boom")).unwrap_err();
        std::panic::set_hook(prev);
        assert_eq!(err, "mermaid renderer panicked: boom");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_renders_supported_state_cycle_with_notes() {
        let _guard = cache_test_lock();
        clear_cache();
        let src = r#"stateDiagram-v2
    [*] --> Ready
    Ready --> Working
    Working --> Ready: loop
    Working --> Done
    Done --> [*]
    note right of Working
        Preserve the current state
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
            "Ready",
            "Working",
            "Done",
            "loop",
            "Preserve the current state",
        ] {
            assert!(output.contains(expected), "missing {expected:?}:\n{output}");
        }
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn run_with_timeout_cancels_cooperative_work() {
        let control = merman::OperationControl::new();
        let render_control = control.clone();
        let cancel_control = control.clone();
        let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
        let result = run_with_timeout(
            Duration::from_millis(50),
            move || cancel_control.cancel(),
            move || {
                while render_control.checkpoint().is_ok() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = cancelled_tx.send(());
                Err("cancelled".to_owned())
            },
        );
        assert!(result.unwrap_err().contains("timed out"));
        cancelled_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should observe cancellation");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn run_with_timeout_passes_fast_results_through() {
        let ok = run_with_timeout(Duration::from_secs(5), || {}, || Ok("done".to_owned()));
        assert_eq!(ok.unwrap(), "done");
        let err = run_with_timeout(Duration::from_secs(5), || {}, || Err("nope".to_owned()));
        assert_eq!(err.unwrap_err(), "nope");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn merman_renders_sequence_self_messages() {
        let _guard = cache_test_lock();
        clear_cache();
        let source = "sequenceDiagram\n    A->>A: wait\n    A->>B: done";
        let output = DefaultMermaidRenderer.render(source).unwrap();
        assert!(output.contains("wait"));
        assert!(output.contains("done"));
    }
}
