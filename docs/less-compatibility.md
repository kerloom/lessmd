# lessmd ↔ `less` compatibility

Snapshot of how `lessmd` lines up with the canonical pager
([`gwsw/less` @ master](https://github.com/gwsw/less), `less-704` / `706`),
and the plan for closing the gaps. Generated from a review of `less.hlp`,
`less.nro.VER`, the `NEWS` file, and the C source.

## Status legend

- ✅ **have** — implemented and tested
- 🟡 **partial** — present but not behaviorally equivalent
- ❌ **missing** — not implemented

## Commands (keybindings)

### ✅ Have

| less | lessmd | Notes |
|---|---|---|
| `e ^E j ^N CR` | `j` `e` `Down` | forward 1 line |
| `y ^Y k ^K ^P` | `k` `y` `Up` | backward 1 line |
| `f ^F ^V SPACE` | `Space` `f` `PgDn` | page down |
| `b ^B ESC-v` | `b` `PgUp` | page up |
| `d ^D` | `Ctrl-D` | half-page down |
| `u ^U` | `Ctrl-U` | half-page up |
| `g < ESC-< HOME` | `g` `Home` | first line |
| `G > ESC-> END` | `G` `End` | last line |
| `ESC-) →` | `l` `→` (8 cols) | pan right |
| `ESC-( ←` | `h` `←` (8 cols) | pan left |
| `h H` | `?` | help (moved) |
| `q :q Q :Q ZZ` | `q` `Q` `Esc` | quit |
| `/pattern` | `/` | forward search |
| `n` | `n` | repeat search |
| `N` | `N` | reverse repeat |
| `r ^R ^L` | `r` `Ctrl-L` | repaint (no-op, ratatui redraws) |
| — | `t` `T` | heading nav (repurposed from less's tag nav) |
| — | `o` | outline overlay |
| — | `Tab` | section folding |

### 🟡 Partial

- `n` / `N` — direction-agnostic; not yet direction-aware (would change once `?` lands).
- Search modifiers (`^N`, `^E`, `^F`, `^K`, `^R`, `^S n`, `^W`, `^L`) — none implemented.

### ❌ Missing

**Moving**

- `ESC-j` / `ESC-k` (file-line vs rendered-line)
- `z` / `w` (set window)
- `ESC-SPACE` / `ESC-b` (don't stop at EOF/BOF)
- `ESC-}` / `ESC-{` / `^→` / `^←` (jump to last/first column)
- `F` (tail -f), `ESC-f` / `ESC-F` (follow + pattern)

**Searching**

- `?pattern` (backward search) — **medium-low LoE**
- `ESC-u` toggle highlight ✅ (just added)
- `ESC-U` clear saved pattern + highlight ✅ (just added)
- `& pattern` (line filter) — **HIGH LoE**
- `^O^N` / `^O^P` / `^O^L` (OSC 8 link nav) — needs OSC 8

**Jumping**

- `p` / `%` (jump to N percent)
- `t` / `T` tags (we use these for headings)
- `{` `(` `[` `}` `)` `]` and `ESC-^F` / `ESC-^B` (bracket matching)
- `m` / `M` / `'` / `''` / `^X^X` / `ESC-m` (marks + go-to + clear)

**Files**

- `:e [file]`, `:n`, `:p`, `:x`, `:d`, `^X^V`
- `= / ^G / :f` (file info)
- Multi-file command-line list — **HIGH LoE**

**Misc**

- `-flag` / `--name` (toggle option at runtime)
- `_flag` / `__name` (query option)
- `!cmd` shell, `#cmd` prompt-expanded shell
- `|X cmd` (pipe to shell)
- `s file` (save input)
- `v` (edit current file via `$VISUAL` / `$EDITOR`)
- `+cmd` (initial command, e.g. `+G`, `+/pattern`) — **medium LoE**

## Command-line options

### ✅ Have

| less | lessmd | Notes |
|---|---|---|
| `-?` / `--help` | `-h` / `--help` | |
| `-V` / `--version` | `-V` / `--version` | |
| `-N` / `--LINE-NUMBERS` | `-N` / `--line-numbers` | |
| `-F` / `--quit-if-one-screen` | `-F` / `--quit-if-one-screen` | ✅ just added |
| `-i` / `--ignore-case` | `-i` / `--ignore-case` | ✅ just added |
| `-I` / `--IGNORE-CASE` | `-I` / `--IGNORE-CASE` | ✅ just added |
| `-g` / `--hilite-search` | `-g` / `--hilite-search` | ✅ just added (only current match) |
| `-G` / `--HILITE-SEARCH` | `-G` / `--HILITE-SEARCH` | ✅ just added (no highlight) |
| — | `--markdown` / `--plain` | ours |
| — | `--no-syntax` / `--no-mermaid` | ours |

### ❌ Missing (by LoE)

**Low** (~1-line flags, ≤20 LOC each)

- `-e` / `--quit-at-eof` (auto-exit on 2nd EOF)
- `-E` / `--QUIT-AT-EOF` (auto-exit on 1st EOF)
- `-K` / `--quit-on-intr` (Ctrl-C exits)
- `-q` / `-Q` / `--quiet` (silence bell)
- `-X` / `--no-init` (skip termcap init/deinit)
- `-c` / `-C` / `--clear-screen` (repaint by clearing)
- `-f` / `--force` (open non-regular files)
- `-d` / `--dumb` (suppress dumb-terminal warning)

**Medium-low** (touches multiple call sites)

- `-s` / `--squeeze-blank-lines`
- `-a` / `--search-skip-screen`
- `-A` / `--SEARCH-SKIP-SCREEN`
- `-m` / `-M` / `--long-prompt` + `-P` (configurable prompt)
- `-n` / `--line-numbers` (suppress in prompts)
- `-j N` / `--jump-target`
- `--hilite-target` / `-DJ`
- `-J` / `--status-column`
- `-# N` / `--shift` (horizontal scroll amount)
- `-x N` / `--tabs` (tab stop)
- `-y N` / `--max-forw-scroll`
- `-h N` / `--max-back-scroll`
- `-z N` / `--window`
- `-w` / `-W` / `--hilite-unread`
- `-p pattern` / `--pattern` (start at pattern) — **medium LoE**
- `-o file` / `-O file` / `--log-file` (log to file)
- `--incsearch` (search as you type)

**Medium**

- `-r` / `-R` / `--raw-control-chars` / `--RAW-CONTROL-CHARS` — we have hand-rolled SGR but not full OSC 8 pass-through
- `-S` / `--chop-long-lines` (chop vs wrap) — **medium LoE**
- `--wordwrap` (wrap at spaces)
- `-t tag` / `-T tagsfile` (tag nav)
- `--mouse` (mouse wheel/click)

**High / architectural**

- `-k file` / `--lesskey-file` (lesskey config)
- `-D xcolor` / `--color` / `--use-color` (configurable colors)
- OSC 8 hyperlink support (`^O^N` etc. + `--mouse` integration)
- `&` filter
- `:e` / multi-file list
- LESSOPEN / LESSCLOSE preprocessor
- LESSHISTFILE / history persistence
- Bracket matching `{` `(` `[` and `ESC-^F`/`^B`
- Marks (`m` / `M` / `'` / `''` / `ESC-m`)
- Tab/edit (vi integration)

## Test methodology

The biggest **methodology** gap is end-to-end TUI testing:

- `less` ships [`lesstest/`](https://github.com/gwsw/less/tree/master/lesstest) —
  a fake terminal (`lt_screen`) plus a scriptable key-by-key driver
  (`lesstest`) that replays `.lt` fixtures (chinese1, colorbars, filter1,
  github216, …) and compares the simulated framebuffer.
- `lessmd` has ~245 unit tests + integration tests + `#[ignore]` perf tests
  on pure `PagerState`, but no TUI harness. Closing this would be a
  separate workstream using `ratatui::backend::TestBackend`.

## LoE-ranked roadmap

**Strictly low LoE (already in this batch or near-trivial):**

- [x] `-F` / `--quit-if-one-screen`
- [x] `-i` / `-I` / `--ignore-case` / `--IGNORE-CASE`
- [x] `-g` / `-G` + `ESC-u` toggle / `ESC-U` clear
- [x] `r` / `Ctrl-L` / `Ctrl-R` repaint
- [ ] Numbered command args (`5j`, `10G`, `50%`, `+/pat-N`) — **next batch**
- [ ] `-e` / `-E` (auto-exit on EOF)
- [ ] `-K` (Ctrl-C exits)
- [ ] `-q` / `-Q` (silence bell)

**Medium-low (deferred per "even skip medium-low"):**

- [ ] `?` backward search
- [ ] `+cmd` / `-p pattern` (initial command)
- [ ] `-S` (chop long lines)
- [ ] `-a` / `-A` (search-skip-screen)
- [ ] `-j N` (jump target)
- [ ] `-m` / `-M` (configurable prompt)
- [ ] `-J` (status column)
- [ ] `-# N` (horizontal scroll amount)
- [ ] `-x N` (tab stops)
- [ ] `-s` (squeeze blank lines)
- [ ] `--incsearch`

**Medium / high (architectural, deferred indefinitely):**

- [ ] End-to-end TUI test harness (`TestBackend`)
- [ ] OSC 8 pass-through + clickable links
- [ ] `:e` / multi-file list
- [ ] `&` line filter
- [ ] Marks (`m` / `'` / etc.)
- [ ] Bracket matching
- [ ] Shell commands (`!` / `#` / `|X`)
- [ ] `-k` lesskey config
- [ ] LESSOPEN / LESSCLOSE preprocessor
- [ ] Tag navigation (`-t` / `t` / `T`)
