# lessmd

A `less`-like terminal pager that renders Markdown, syntax-highlighted code blocks, and Mermaid diagrams. Plain text works too, with ANSI colors passed through by default (like `less -R`).

Single static binary, no scripting runtimes. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui) and [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark).

## Installation

**Pre-built binary** (Linux, macOS, Windows):

```sh
curl -fsSL https://raw.githubusercontent.com/kerloom/lessmd/master/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/kerloom/lessmd/master/install.ps1 | iex
```

Optional env vars: `LESSMD_VERSION`, `LESSMD_INSTALL`, `LESSMD_REPO`.

**From source** (requires Rust 1.85+):

```sh
cargo install --git https://github.com/kerloom/lessmd
```

Or clone and build:

```sh
git clone https://github.com/kerloom/lessmd.git
cd lessmd
cargo build --release
# binary: target/release/lessmd
```

On macOS, after copying the binary to `~/.local/bin`, re-sign it if the shell kills it with `Code Signature Invalid`:

```sh
xcrun codesign --force --sign - ~/.local/bin/lessmd
```

## Features

- **Markdown rendering** — headings, emphasis, lists, blockquotes, tables, links, fenced code
- **Syntax highlighting** — fenced code blocks via syntect (disable with `--no-syntax`)
- **Mermaid diagrams** — inline ASCII/Unicode rendering via figurehead (disable with `--no-mermaid`)
- **Less-compatible navigation** — scroll, search, goto line/percent, digit prefixes (`5j`, `10G`)
- **Markdown extras** — heading jump (`t`/`T`), outline (`o`), fold headings (`Tab`), table pan (`w`/`h`/`l`)
- **Plain text mode** — auto-detect by extension (`.md`/`.markdown`), or force with `--markdown` / `--plain`
- **Search** — forward/backward over rendered text; case modes and highlight options mirror `less`

## Usage

```sh
lessmd README.md                  # markdown (by extension)
lessmd --plain logs.txt           # plain text, strip ANSI
lessmd --markdown notes.txt       # force markdown on non-.md file
cat file.md | lessmd -            # stdin

lessmd -N docs/guide.md           # line numbers
lessmd +G CHANGELOG.md            # start at bottom
lessmd +/pattern file.md          # start at first match
lessmd -p pattern file.md         # same as +/pattern
lessmd -F small.txt               # exit if fits on one screen
```

Press `H` inside the pager for keybindings, or run `lessmd --help`.

See [docs/lessmd.md](docs/lessmd.md) for a fuller guide and [docs/less-compatibility.md](docs/less-compatibility.md) for `less` flag parity.

## Development

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Build without optional features:

```sh
cargo build --no-default-features
cargo test --no-default-features
```

`PagerState` and rendering logic are pure (no terminal I/O outside `main.rs`), so most behavior is covered by unit tests. See [AGENTS.md](AGENTS.md) for the full verify checklist and release process.

## Cargo features

| Feature | Default | Description |
| --- | --- | --- |
| `syntax` | yes | Fenced-code syntax highlighting (syntect) |
| `mermaid` | yes | Inline Mermaid rendering (figurehead) |

## License

MIT
