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
| `cargo clippy --all-targets -- -D warnings` | Lint; warnings are errors |
| `cargo clippy --all-targets --no-default-features -- -D warnings` | Lint the minimal build |
| `cargo test` | Run the full test suite |
| `cargo test --no-default-features` | Run the minimal test suite without syntax/Mermaid deps |
| `cargo run -- [path]` | Run the pager on a file or stdin (`-` = stdin) |
| `cargo run -- --no-syntax --no-mermaid [path]` | Runtime-disable enhancements |
| `xcrun codesign --force --sign - ~/.local/bin/lessmd` | Re-sign the installed binary after copy (macOS rejects the build's path-embedded ad-hoc signature, causing `SIGKILL` with `Code Signature Invalid`) |

## Release (GitHub Actions)

Semi-automatic: you bump `version` in `Cargo.toml`, push to `master`, and the workflow releases if the version changed.

1. Bump `version` in `Cargo.toml` and merge/push to `master`.
2. The workflow runs when `Cargo.toml` changes, detects the version bump, and publishes `v{version}` with six pre-built binaries.

Skipped automatically when:
- `Cargo.toml` changed but the version line did not
- a GitHub Release for that tag already exists

To retry or override: *Actions → Release → Run workflow* with **force** (optional **draft**).

Build targets: x86_64 + aarch64 for Linux, macOS, and Windows. Each release includes archives and `SHA256SUMS`.

## Pre-completion checklist

Before marking any milestone task complete, all of these must pass:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test
cargo test --no-default-features
```

## Conventions

- **Language:** Rust, edition 2024.
- **Deps:** keep minimal — `ratatui`, `crossterm`, `pulldown-cmark` (M2),
  `figurehead` behind a `mermaid` feature (M3), `syntect` behind a `syntax`
  feature (M5). No clap yet (hand-rolled args).
- **Architecture:** `PagerState` (and all pager logic) must stay pure — no
  terminal I/O outside `main.rs`. This keeps it unit-testable.
- **Tests:** per-phase, not at the end. Each new feature gets its own unit
  test in the module that implements it, plus an integration fixture when
  useful. See `plan.md` for the per-phase test list.
- **Comments:** only where non-obvious (module-level docs ok; avoid inline
  noise). This matches the repo's code style.
- **Plan & tracker:** update `plan.md` (Session Tracker + per-task checklist)
  at the end of each session.
