# lessmd — Plan & Tracker

A `less`-like terminal pager that renders markdown (and mermaid, in phase 2).
Built from scratch in Rust for simplicity, minimal dependencies, and readable code.

## Decisions

- **Build from scratch in Rust** — don't fork `less` (C / GPL / autotools; adding a
  markdown viewport is invasive; the `LESSOPEN` preprocessor route is a stopgap, not a
  first-class product). Don't use Zig (no mature TUI ecosystem — would reinvent ratatui).
- **Stack:** `ratatui` + `crossterm` + `pulldown-cmark` (phase 1) + `figurehead` (phase 2).
  All MIT / lean, pure Rust, static binary, no scripting runtimes, no system libraries.
- **Markdown:** custom ~300–500 line renderer on top of pulldown-cmark events. Every line
  is ours; minimal deps; full control.
- **Mermaid:** `figurehead` (pure-Rust ASCII/Unicode) behind a swappable trait, with
  graceful fallback to a plain code block on error.
- **Pre-render to `Vec<Line<'static>>`** — the whole document is rendered up-front to a
  flat vector of terminal rows (already wrapped to width). The pager just slices a window
  over it. Scroll/search/resize become trivial. Streaming for huge files is a later concern.
- **Pure pager logic** — `PagerState` has no terminal I/O and is unit-testable. `main.rs`
  is the only place that touches crossterm/ratatui directly.
- **Tests per phase, not at the end** — each milestone ships its own unit + integration
  tests (see per-phase test sections below).

## Resolved decisions (from plan review)

- Q1 — Name & binary: `lessmd` (crate + bin name).
- Q2 — Arg parser: hand-rolled for now (no clap). Revisit only if flags grow.
- Q3 — Markdown detection: extension-based (`.md`/`.markdown` = markdown, else text)
  overridable with `--markdown` / `--plain`. No content heuristic.
- Q4 — Search scope: search the *rendered* text (matches `less`+preprocessor behavior;
  jumps land on rendered rows).
- Q5 — ANSI in plain text: passthrough on by default (`less -R` behavior); `--plain`
  strips it.

## Architecture

```
lessmd/
├── Cargo.toml
├── README.md
├── AGENTS.md                  # build/test/lint commands for future sessions
├── plan.md                    # this file
├── src/
│   ├── main.rs                # CLI entry, terminal setup/teardown, event loop
│   ├── cli.rs                 # arg parsing (hand-rolled initially; clap later if needed)
│   ├── source.rs              # resolve input: file path or stdin; detect markdown
│   ├── document.rs            # Document { lines: Vec<Line<'static>>, ... }
│   ├── pager.rs               # PagerState: offset, height, width, search — pure logic
│   ├── search.rs              # incremental/regex search over rendered lines
│   ├── input.rs               # less-like keybinding dispatch
│   ├── help.rs                # help overlay
│   └── render/
│       ├── mod.rs             # Renderer trait + dispatch (text vs markdown)
│       ├── text.rs            # plain text → Vec<Line>, optional ANSI pass-through
│       ├── markdown.rs        # pulldown-cmark events → Vec<Line>  (phase 1)
│       └── mermaid.rs         # MermaidRenderer trait + figurehead impl (phase 2)
└── tests/
    ├── fixtures/
    │   ├── plain.txt
    │   ├── headings.md
    │   ├── inline.md
    │   ├── lists.md
    │   ├── blockquote.md
    │   ├── tables.md
    │   ├── codeblocks.md
    │   └── mermaid.md
    └── integration.rs
```

### Module responsibilities (kept small)

- **`source.rs`** — read file or stdin into a `String`; detect markdown (extension
  `.md`/`.markdown`, or `--markdown`/`--plain` flag). Returns `Input { text, is_markdown }`.
- **`render/text.rs`** — split into lines, optional ANSI passthrough (default on,
  like `less -R`), wrap to width. ~50 lines.
- **`render/markdown.rs`** — iterate `pulldown::Parser` events, maintain a small stack
  of inline style state, emit `Line`s. Re-wrap on width change by re-running render.
- **`render/mermaid.rs`** — `MermaidRenderer` trait + `FigureheadRenderer` impl +
  fallback to source-as-codeblock on error.
- **`document.rs`** — holds `lines: Vec<Line<'static>>` and `source_path`. Methods:
  `render(Input, width) -> Self`, `line_count()`, `slice(offset, height)`.
- **`pager.rs`** — `PagerState { doc, offset, height, width, search, quit }`. One
  `handle_key` method mutates state. Pure logic, no I/O.
- **`input.rs`** — maps keys to `PagerState` actions (less-compatible set).
- **`main.rs`** — setup crossterm raw mode + alternate screen, event loop, draw
  `ratatui::Frame` from `pager.slice()`, teardown on panic/exit. ~80 lines.

### Less-compatible keybindings

`j`/`e`/`Ctrl-N` down · `k`/`y`/`Ctrl-P` up · `Space`/`f` page down · `b` page up ·
`g` top · `G` bottom · `q`/`Q` quit · `h`/`H` help · `/` search · `n`/`N` next/prev
match · `Ctrl-C` abort search · arrows/mouse wheel optional (phase 3).

## Dependencies (initial `Cargo.toml`)

```toml
[dependencies]
ratatui = "0.30"          # TUI toolkit (MIT)
crossterm = "0.28"        # terminal backend (MIT)
pulldown-cmark = "0.13"   # markdown parser (MIT) — phase 1
figurehead = "0.4"        # mermaid → ASCII (MIT) — phase 2, feature-gated

[profile.release]
lto = true
strip = true
```

## Verification commands (CI + local)

```
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo test --features mermaid      # once phase 2 lands
```

(These will also be recorded in `AGENTS.md` so future sessions know to run them.)

---

## Phased Delivery & Tests

Tests ship with each phase, not at the end. Each phase below lists its own unit and
integration tests.

### Phase 0 — Skeleton

**Scope:** Cargo project, crossterm alt-screen hello-world, clean teardown on panic.

**Tests (phase 0):**
- `cargo build` succeeds.
- Unit: `main` exits cleanly (smoke — guarded behind a `#[cfg(test)]` helper that does
  not actually enter raw mode).

**Exit criteria:** `cargo run -- README.txt` opens the alternate screen, prints
"hello", and `q` quits without leaving the terminal in a broken state.

### Phase 1a — Plain-text pager (M1)

**Scope:** read file/stdin, render plain text, scroll, search, resize, help.

**Tests (phase 1a):**
- Unit (`render/text.rs`):
  - long line wraps to given width
  - ANSI passthrough preserves a simple `\x1b[31mred\x1b[0m` span when enabled
  - ANSI passthrough stripped when `--plain` (no-ansi) mode set
  - empty input → empty `Document`
- Unit (`pager.rs`):
  - scroll down stops at last line (no overshoot)
  - scroll up stops at 0
  - page-down / page-up move by viewport height
  - `g` → offset 0; `G` → offset = max(0, line_count - height)
  - resize updates height/width and re-renders; offset clamped into valid range
  - search sets matches; `n` advances; `N` retreats; wraps around
  - edge cases: empty doc, 1-line doc, doc shorter than viewport
- Integration (`tests/integration.rs`):
  - render `fixtures/plain.txt`, assert line count matches input line count (no wrap)
    and matches content substring for first/last line
- Manual smoke: `lessmd README.txt` behaves like `less README.txt` for core keys.

**Exit criteria:** plain-text parity with `less` for the core keybindings above.

### Phase 1b — Markdown rendering (M2)

pulldown-cmark with GFM: `ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRIKETHROUGH |
ENABLE_SMART_PUNCTUATION`.

**Features (ordered by value):**
1. Headings H1–H6 (bold + color scale + underline for H1/H2)
2. Paragraphs + soft/hard breaks
3. Inline: **bold**, *italic*, `code`, ~~strike~~, [links](url) (styled; OSC8 deferred)
4. Fenced code blocks (language label shown; no syntax highlighting yet)
5. Bullet & ordered lists (nested, proper indent)
6. Task lists (☐/☑)
7. Blockquotes (left bar + dim)
8. Horizontal rules
9. GFM tables
10. Images: show alt text + URL (no fetching)

**Deferred:** footnotes, math, raw HTML (escape/skip), syntax highlighting.

**Tests (phase 1b) — added incrementally as each feature lands:**
- Unit (`render/markdown.rs`), one test per feature:
  - `renders_h1_with_bold_and_color` — asserts bold + expected style flag
  - `renders_paragraph_and_softbreak` — asserts two `Line`s, soft break joins with space
  - `renders_bold_italic_code_strike` — each inline produces correct `Span` styles
  - `renders_link_styled_with_url_visible` — link text styled, URL shown
  - `renders_fenced_codeblock_with_language_label` — label line + indented body
  - `renders_nested_bullet_list` — indent depth per level
  - `renders_ordered_list_with_correct_numbers`
  - `renders_tasklist_checked_and_unchecked` — ☑ / ☐ present
  - `renders_blockquote_with_left_bar`
  - `renders_horizontal_rule` — a line of `─`
  - `renders_gfm_table_header_separator_alignment`
  - `renders_image_as_alt_text_plus_url`
  - `skips_or_escapes_raw_html`
- Integration (`tests/integration.rs`):
  - `fixtures/headings.md` → assert H1..H6 each present with expected text
  - `fixtures/inline.md` → assert bold/italic/code/strike/link text present
  - `fixtures/lists.md` → assert nested indent + tasklist markers
  - `fixtures/blockquote.md` → assert left-bar prefix on rendered lines
  - `fixtures/tables.md` → assert table row count and a header cell substring
  - `fixtures/codeblocks.md` → assert language label and body content
- Manual smoke: `lessmd README.md` renders the project README readably.

**Exit criteria:** all phase-1b features render correctly; full test suite green.

### Phase 2 — Mermaid (M3)

**Scope:** intercept ```` ```mermaid ```` blocks during markdown render; render via
`figurehead`; fallback to source-as-codeblock on error.

**Tests (phase 2):**
- Unit (`render/mermaid.rs`):
  - `detects_mermaid_fenced_block` — parser routes block to `MermaidRenderer`
  - `figurehead_renders_simple_flowchart` — a 2-node flowchart yields >= 1 non-empty
    `Line` containing box-drawing chars
  - `figurehead_renders_sequence_diagram` — yields lines containing `─`/`▶` or arrows
  - `renders_unsupported_diagram_as_codeblock_fallback` — on `Err`, output equals the
    plain codeblock render plus a dim note line
  - `renders_invalid_mermaid_as_codeblock_fallback` — syntactically invalid source →
    fallback path, no panic
  - `mermaid_renderer_trait_is_swappable` — a `MockRenderer` returns canned output and
    the markdown renderer uses it (trait dispatch works)
- Integration (`tests/integration.rs`, behind `--features mermaid`):
  - `fixtures/mermaid.md` (mixed markdown + a flowchart + a sequence diagram) →
    assert diagram box-drawing characters present and surrounding markdown intact
  - `fixtures/mermaid.md` with an invalid block → assert fallback note substring present
- Manual smoke: `lessmd fixtures/mermaid.md` shows rendered diagrams inline.

**Exit criteria:** flowchart + sequence diagrams render in-terminal; errors degrade
gracefully; full suite (including `--features mermaid`) green.

### Phase 3 — Polish (optional, post-MVP)

**Scope (pick as needed):** syntax highlighting (syntect is heavy — evaluate a tiny
tree-sitter subset or skip), OSC8 clickable hyperlinks, `--line-numbers` toggle,
jump-to-heading (`t`/`T` or `:n`), section folding, lesskey-style config file,
streaming for very large files (chunked render).

**Tests (phase 3):** added per-feature as each lands (one unit + one integration per
feature, following the phase 1b/2 pattern). No "big test drop" at the end.

---

## Session Tracker

Update this table at the end of each session. Mark items `[x]` when done and verified
(`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test` all green).

### Milestone progress

| Milestone | Scope | Status | Session | Notes |
|---|---|---|---|---|
| M0 — Skeleton | Cargo project, alt-screen hello-world, clean teardown | [x] done | 1 | fmt/clippy/test green; uses `ratatui::run` (init+restore+panic hook) |
| M1 — Plain-text pager | source + render/text + pager + input + search + help | [x] done | 1 | lib+bin split; 46 unit + 4 integration tests; hand-rolled ANSI SGR parser; substring search |
| M2 — Markdown | render/markdown for all phase-1b features + per-feature tests | [x] done | 1 | pulldown-cmark 0.13; 21 unit + 7 integration markdown tests; tables/lists/blockquotes/code/inline all render |
| M3 — Mermaid | render/mermaid + figurehead + fallback + tests | [x] done | 2 | optional `figurehead`; swappable renderer trait; valid diagrams render with `--features mermaid`; disabled/error paths fall back to code block + note; fmt/clippy/test + feature test green |
| M4 — Polish | highlights / OSC8 / line-numbers / config (optional) | [x] done | 7 | Jump-to-heading (outline overlay `o`, `t`/`T` keys), `--line-numbers` / `-N` flag, section folding (`Tab` key with visible-line-map). OSC8 deferred (ratatui 0.30 has no hyperlink support). 129 unit + 12 integration tests green. |
| M5 — Syntax highlighting | syntect behind `syntax` feature, highlight cache | [x] done | 9 | `syntect 5.3` with `default-fancy` (pure-Rust `fancy-regex`), `base16-ocean.dark` theme. `highlight_code()` in `src/render/syntax.rs` with `LazyLock<SyntaxSet/ThemeSet>`. Highlight cache (`HashMap<(lang, code), Vec<Line>>`, max 256 entries) so resize re-wraps instead of re-highlights (~6 µs/block vs ~150 µs/block, ~30× faster resize). 60 syntax unit tests (25 languages + aliases + cache behavior + style mapping). 2 perf tests for render + resize with cache. 192 unit + 12 integration + 14 perf green with `--features syntax`. |
| M6 — Instant first paint | default enhancements + plain-first background render | [x] done | 10 | Cargo defaults now include `syntax` + `mermaid`; runtime flags `--no-syntax` / `--no-mermaid` disable enhancements. Markdown opens with syntax/Mermaid off, draws immediately, then swaps in a background enhanced render. Startup perf (`perf_startup_two_phase_render`): 10 blocks 147ms full enhanced → 180µs plain-first; 100 blocks 76ms → 399µs; 500 blocks 5.5ms → 2.0ms. |
| M7 — Visible-first enhancement | enhanced initial viewport overlay | [x] done | 11 | Background worker now renders an enhanced prefix (`terminal_height * 20` source lines) and overlays the top viewport before the full enhanced document is ready. Overlay is only used at `offset == 0` and is cleared when the full document swaps in. Helps syntax and Mermaid in the first screen appear earlier without blocking first paint. Added pager overlay tests and `perf_startup_viewport_overlay_render`. |
| M8 — Prefix streaming first paint | prefix initial render for all inputs | [x] done | 12 | Initial render now uses only a source prefix for large inputs (plain text or markdown), then swaps in the full document from the background. This improves first paint for the whole program, not just syntax/Mermaid. Perf (`perf_prefix_first_paint_render`): 100k-line plain text 279.6ms full → 1.4ms prefix (~203×); 3.3MB markdown 131.5ms full → 1.2ms prefix (~107×). |
| M9 — Background safety + Mermaid cache | stale render discard, resize respawn, Mermaid render cache | [x] done | 13 | Background render messages now carry a generation id; stale viewport/full messages are discarded after resize, and enhancement work is respawned at the new terminal size when needed. Resizing clears viewport overlays so old wrapping cannot persist. Mermaid rendering now has a bounded 128-entry cache keyed by source, caching both success and failure results for duplicate diagrams and resize re-renders. |
| M10 — Review hardening | low-effort/high-benefit review fixes | [x] done | 16 | Fixed stale keybinding docs/help; made syntax/Mermaid defaults feature-aware; reset fold/outline state on document replacement; made highlighting respect search case mode; sanitized status and markdown-derived terminal text. |

### Session log

| Session | Date | Done this session | Next session pickup |
|---|---|---|---|
| 1 | 2026-06-19 | M0 skeleton; M1 plain-text pager; M2 markdown renderer (headings, inline bold/italic/code/strike, links, fenced+indented code blocks, nested lists, ordered lists, task lists, blockquotes, horizontal rules, GFM tables, images, raw HTML). fmt+clippy+test green (65 unit + 11 integration). Smoke test confirms rendering. | M3: add `figurehead` behind `mermaid` feature; `render/mermaid.rs` trait + figurehead impl + fallback; intercept ```` ```mermaid ```` blocks; per-feature tests + `fixtures/mermaid.md`. |
| 2 | 2026-06-19 | M3 Mermaid support: optional `figurehead`, swappable renderer trait, Mermaid fenced-block interception, fallback note, fixture/tests, panic-safe renderer, and sequence self-message retry sanitizer. fmt+clippy+test green with and without `--features mermaid`. | Improve wide diagram behavior: unwrapped Mermaid output plus horizontal panning/clipping. |
| 3 | 2026-06-19 | Wide diagram polish: Mermaid output no longer wraps, viewport clips by terminal cell, Left/Right pan horizontally, status shows `col current/max`, help updated, and tests added for clipping/style preservation/keybindings/no-wrap Mermaid. fmt+clippy+test green with and without `--features mermaid`. | Optional polish: compact diagram labels or general horizontal panning for more preformatted blocks if needed. |
| 4 | 2026-06-19 | Usability polish: `h`/`l` also pan horizontally, help moved to `?`, status bar shows dim `? help`, and wide Mermaid diagrams emit an inline dim pan hint. fmt+clippy+test green with and without `--features mermaid`. | Optional polish: compact diagram labels or general horizontal panning for more preformatted blocks if needed. |
| 5 | 2026-06-19 | Contrast polish: status/help and Mermaid hint/fallback grays use brighter `Gray` instead of dim `DarkGray`; Mermaid pan hint changed to `Use <-/-> or h/l to pan`. fmt+clippy+test green with and without `--features mermaid`. | Optional polish: compact diagram labels or general horizontal panning for more preformatted blocks if needed. |
| 6 | 2026-06-19 | Help overlay behavior: `q`, `Q`, `Esc`, and `?` close help without quitting; help key labels use ASCII arrow notation (`v`, `^`, `<-`, `->`). fmt+clippy+test green with and without `--features mermaid`. | Optional polish: compact diagram labels or general horizontal panning for more preformatted blocks if needed. |
| 7 | 2026-06-19 | M4 polish: heading index during markdown render (`RenderOutput` with `Vec<Heading>`); outline overlay (`o` key) with selection + jump; `t`/`T` next/prev heading navigation; `--line-numbers` / `-N` CLI flag with two-pass gutter width; section folding (`Tab` key) with `visible_indices` map, `jump_to_doc_line` auto-unfold, and `▸`/`▾` fold indicators. OSC8 deferred (ratatui 0.30 lacks hyperlink support). fmt+clippy+test green with and without `--features mermaid` (129 unit + 12 integration). | Streaming for very large files; syntax highlighting; remaining polish as needed. |
| 8 | 2026-06-19 | Search highlighting (current match in yellow, other matches in dark gray); outline overlay height increased to 90% screen + 70% width; separate performance test suite (`tests/perf.rs` with 8 `#[ignore]` tests measuring CPU, memory expansion, scroll, search, and resize). Fixed resize bug where stale `visible_indices` caused OOB panic. fmt+clippy+test green with and without `--features mermaid` (132 unit + 12 integration + 8 perf). | Streaming for very large files; syntax highlighting; remaining polish as needed. |
| 9 | 2026-06-19 | M5 syntax highlighting: `syntect 5.3` behind `syntax` feature (pure-Rust `fancy-regex`, `default-fancy` + `default-themes`); `src/render/syntax.rs` with `highlight_code()` using `LazyLock<SyntaxSet/ThemeSet>` and `base16-ocean.dark` theme; language alias normalization (rust→rs, python→py, typescript→js fallback, etc.); highlight cache (`HashMap<(lang, code), Vec<Line>>`, max 256 entries) so resize re-wraps instead of re-highlights (~6 µs/block vs ~150 µs/block, ~30× faster resize for code-heavy docs); `push_code_block` in `markdown.rs` uses highlighting when feature enabled, falls back to plain yellow code when disabled or language unknown. 60 syntax unit tests (25 languages + aliases + 9 cache behavior tests + style mapping). Added `tests/fixtures/languages.md` multi-language fixture and syntax integration tests. 3 perf tests for init, render, and resize with cache. Binary: 951 KB → 2.89 MB. Init perf: syntax/theme load ~0.7ms median, first highlight ~15–16ms median. fmt+clippy+test green across all 4 feature combos (132/192/138/198 unit + 12-14 integration + 12-15 perf). | Streaming for very large files; custom syntax dump (reduce binary); remaining polish. |
| 10 | 2026-06-19 | M6 instant first paint: default Cargo features now include syntax + Mermaid; CLI flags `--no-syntax` and `--no-mermaid`; `RenderOptions` threaded through render/document/pager; markdown runtime options disable syntax/Mermaid without changing Cargo features; `main.rs` renders markdown once with both enhancements off, enters the UI, then background-renders the enhanced document and swaps it in while preserving pager state. Fixed plain-code styling so wrapping preserves yellow spans. Added startup comparison perf test. Verified `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `cargo test`, and `cargo test --no-default-features`. | Lazy visible enhancement of code/Mermaid blocks, then streaming renderer for whole program. |
| 11 | 2026-06-19 | M7 visible-first enhancement: the background worker sends an enhanced initial-viewport overlay before the full enhanced document; `PagerState::viewport_overlay` is used only at top-of-file and cleared on full doc swap. Added unit tests for overlay replacement/top-only behavior and prefix input construction. Added `perf_startup_viewport_overlay_render`. Verified default and no-default clippy/tests. | Streaming renderer for whole program. |
| 12 | 2026-06-19 | M8 prefix streaming first paint: initial `PagerState` now renders a source prefix for large inputs in all modes, then background-renders and swaps in the full document. The full input is kept on `PagerState` so resize/search use the real source after startup. Added tests for prefix decision/scaling and `perf_prefix_first_paint_render`. Verification green with default and `--no-default-features`. | True on-demand scrolling renderer if needed; current prefix streaming solves first-paint latency while preserving existing pager model. |
| 13 | 2026-06-19 | Added generation-aware background rendering: viewport/full messages include a generation id and stale messages are ignored after resize; resize clears viewport overlays and respawns enhancement work at the new dimensions when the state is still plain/prefix-rendered. Added bounded Mermaid cache (`HashMap<source, Result<String, String>>`, max 128 entries, clear on overflow) in `DefaultMermaidRenderer`, plus cache hit/overflow tests. Verified `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `cargo test`, and `cargo test --no-default-features`. | Consider background status UX or chunked full-render progression next; true on-demand renderer remains the larger architecture step. |
| 14 | 2026-06-19 | Removed the "enhanced viewport ready" / "enhanced render ready" status messages set after the two-phase background render. They permanently overrode the file-name + `xx%` + `? help` status line, hiding the percentage and help hint. Background swaps are now transparent — the visual indicator is the content changing (syntax colors / Mermaid diagrams appearing). fmt + clippy + test + perf green with default and `--no-default-features`. | None outstanding. |
| 15 | 2026-06-19 | Headings (document + outline): each level now renders with a circled-number icon (① ② ③ ④ ⑤ ⑥) in the level color and standard markdown indent (H1 at leftmost, H2 = 2 spaces, …, H6 = 10 spaces). The document uses a new `flush_heading` so wrapped continuation lines stay indented (no icon on continuation). Outline overlay keeps the same number + standard indent. Updated unit tests for `renders_h1_with_bold_and_color`, `renders_all_heading_levels`, `heading_index_captures_levels_and_text`, and added `heading_icon_and_indent_match_level`. fmt + clippy + test (210 + 7 + 14) green with default and `--no-default-features`. | None outstanding. |
| 16 | 2026-06-20 | Review hardening pass: corrected keybinding docs/help; made runtime and library enhancement defaults feature-aware; cleared stale folded heading indices and clamped outline selection on document replacement; made search highlighting use `CaseMode`; sanitized status/search/file-name strings; sanitized markdown text/code/link/Mermaid output before rendering. Added regression tests for feature defaults, replace-doc fold state, case-aware highlighting, and terminal-control stripping. | Remaining review items: binary-level CLI tests, input size safeguards, idle redraw optimization, resize coalescing, and perf/cache refinements. |
| 17 | 2026-06-20 | Added binary-level CLI integration tests for `--help`, `--version`, unknown options, and source-read errors. These cover paths that exit before terminal setup and run under both default and `--no-default-features` builds. | Remaining review items: input size safeguards, idle redraw optimization, resize coalescing, and perf/cache refinements. |

### Per-task checklist (granular tracker)

Copy the relevant block into "In progress" at the start of a session and tick items
off as you go. Verification = `cargo fmt --check && cargo clippy -- -D warnings &&
cargo test` (add `--features mermaid` from M3 on).

#### M0 — Skeleton
- [x] `cargo init --name lessmd`
- [x] add `ratatui` + `crossterm` deps
- [x] `src/main.rs`: enter alt screen, print "hello", `q` quits, restore terminal on
      panic (use a guard struct + `Drop`)
- [x] `cargo build` green
- [x] record build/test/lint commands in `AGENTS.md`

#### M1 — Plain-text pager
- [x] `src/cli.rs`: hand-rolled arg parse (file path or `-` for stdin, `--markdown`,
      `--plain`, `--version`, `--help`)
- [x] `src/source.rs`: read file/stdin → `Input { text, is_markdown }`
- [x] `src/render/text.rs`: split + wrap + optional ANSI passthrough
  - [x] tests: wrap, ansi-passthrough, ansi-stripped, empty-input
- [x] `src/document.rs`: `Document::render`, `line_count`, `slice`
- [x] `src/pager.rs`: `PagerState` + `handle_key` (pure logic)
  - [x] tests: scroll bounds, page up/down, g/G, resize clamp, search n/N wrap, edge cases
- [x] `src/search.rs`: incremental + regex search over rendered lines
- [x] `src/input.rs`: key → action dispatch
- [x] `src/help.rs`: help overlay
- [x] wire `main.rs` event loop + draw
- [x] `tests/integration.rs`: plain.txt render asserts
- [ ] manual smoke vs `less`
- [x] verification commands green

#### M2 — Markdown
- [x] add `pulldown-cmark` dep; enable GFM options
- [x] `src/render/markdown.rs` skeleton + `Renderer` dispatch in `render/mod.rs`
- [x] feature: headings + test
- [x] feature: paragraphs + soft/hard breaks + test
- [x] feature: inline bold/italic/code/strike + test
- [x] feature: links (styled, URL visible) + test
- [x] feature: fenced code blocks (language label) + test
- [x] feature: nested bullet lists + test
- [x] feature: ordered lists + test
- [x] feature: task lists + test
- [x] feature: blockquotes + test
- [x] feature: horizontal rules + test
- [x] feature: GFM tables + test
- [x] feature: images as alt+URL + test
- [x] raw HTML: escape/skip + test
- [x] integration fixtures: headings, inline, lists, blockquote, tables, codeblocks
- [x] manual smoke: `lessmd README.md`
- [x] verification commands green

#### M3 — Mermaid
- [x] add `figurehead` dep behind `[features] mermaid = ["dep:figurehead"]`
- [x] `src/render/mermaid.rs`: `MermaidRenderer` trait + default figurehead renderer
- [x] intercept ```` ```mermaid ```` in `render/markdown.rs`
- [x] fallback to codeblock + dim note on `Err` / unsupported type / renderer panic
- [x] tests: detection, flowchart, sequence, unsupported fallback, invalid fallback,
      trait swappable (mock)
- [x] integration fixture: `fixtures/mermaid.md` (mixed + valid + invalid)
- [x] manual smoke: `lessmd fixtures/mermaid.md`
- [x] verification green with `--features mermaid`

#### M4 — Polish (optional; tick only what's in scope)
- [x] jump-to-heading (heading index, outline overlay `o`, `t`/`T` keys)
- [x] `--line-numbers` / `-N` flag with left gutter
- [x] section folding (`Tab` key with visible-line-map)
- [ ] OSC8 clickable hyperlinks (deferred — ratatui 0.30 has no hyperlink support)
- [x] syntax highlighting (moved to M5)
- [ ] lesskey-style config file
- [ ] streaming for very large files (next session)
- [x] per-feature tests added as each lands

#### M5 — Syntax highlighting
- [x] add `syntect` dep behind `[features] syntax = ["dep:syntect"]`
- [x] `src/render/syntax.rs`: `highlight_code(code, lang)` → `Option<Vec<Line>>`
- [x] `LazyLock<SyntaxSet>` + `LazyLock<ThemeSet>` (loaded once, reused)
- [x] `base16-ocean.dark` theme
- [x] language alias normalization (rust→rs, python→py, typescript→js, etc.)
- [x] style mapping: `syntect::highlighting::Style` → `ratatui::style::Style`
- [x] wire into `push_code_block` in `markdown.rs` (behind `#[cfg(feature = "syntax")]`)
- [x] fall back to plain yellow code when feature disabled or language unknown
- [x] highlight cache: `HashMap<(lang, code), Vec<Line>>` (max 256, clear on overflow)
- [x] `clear_cache()` function for tests/document switches
- [x] unit tests: 25 languages + aliases + 9 cache behavior tests + 3 style mapping tests
- [x] perf tests: `perf_syntax_highlight_render` + `perf_syntax_highlight_resize_with_cache`
- [x] perf test: `perf_syntax_init_time` (syntax/theme load + first highlight)
- [x] integration fixture: `tests/fixtures/languages.md` with many language blocks
- [x] verification green with `--features syntax` and `--features syntax,mermaid`

## Rough effort

| Milestone | Effort |
|---|---|
| M0 | 0.5 day |
| M1 | 2–3 days |
| M2 | 3–4 days |
| M3 | 1–2 days |
| M4 | optional |
| M5 | 1 day |
