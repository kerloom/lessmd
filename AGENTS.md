# AGENTS.md — lessmd

Reference for future sessions (and for the opencode assistant) on how this
project is built and verified. Always run these before considering a task done.

## Build / verify commands

| Command | Purpose |
|---|---|
| `cargo build` | Compile the crate |
| `cargo build --release` | Release build (LTO + strip per `Cargo.toml`) |
| `cargo fmt --check` | Verify formatting is clean |
| `cargo fmt` | Apply formatting |
| `cargo clippy -- -D warnings` | Lint; warnings are errors |
| `cargo test` | Run the full test suite |
| `cargo test --features mermaid` | Run tests including the mermaid feature (from M3) |
| `cargo run -- [path]` | Run the pager on a file or stdin (`-` = stdin) |

## Pre-completion checklist

Before marking any milestone task complete, all of these must pass:

```sh
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

(From M3 onward, also run `cargo test --features mermaid`.)

## Conventions

- **Language:** Rust, edition 2024.
- **Deps:** keep minimal — `ratatui`, `crossterm`, `pulldown-cmark` (M2),
  `figurehead` behind a `mermaid` feature (M3). No clap yet (hand-rolled args).
- **Architecture:** `PagerState` (and all pager logic) must stay pure — no
  terminal I/O outside `main.rs`. This keeps it unit-testable.
- **Tests:** per-phase, not at the end. Each new feature gets its own unit
  test in the module that implements it, plus an integration fixture when
  useful. See `plan.md` for the per-phase test list.
- **Comments:** only where non-obvious (module-level docs ok; avoid inline
  noise). This matches the repo's code style.
- **Plan & tracker:** update `plan.md` (Session Tracker + per-task checklist)
  at the end of each session.
