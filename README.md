# lazymd

[![Build status](https://github.com/haynescd/lazymd/actions/workflows/ci.yml/badge.svg)](https://github.com/haynescd/lazymd/actions/workflows/ci.yml)
[![Version](https://img.shields.io/github/v/release/haynescd/lazymd)](https://github.com/haynescd/lazymd/releases/latest)

A fast terminal Markdown previewer, written in Rust. Point it at a file and get
a live, styled render right in your terminal — edit in your editor of choice,
save, and the preview updates instantly.

> **Why "lazymd"?** In the spirit of [lazygit](https://github.com/jesseduffield/lazygit):
> a keyboard-driven terminal tool that saves you a trip somewhere else — here,
> the browser. Open a file once and stay lazy; every save shows up on its own.
> (Started as a side project to learn Rust.)

## Features

- **GitHub Flavored Markdown** via [`comrak`](https://crates.io/crates/comrak):
  tables, task lists, strikethrough, autolinks, footnotes, and `> [!NOTE]`-style
  alerts.
- **Real styling, not raw syntax** — bold is bold, headings are colored with no
  `#`, lists get `•`/`◦`/`▪` bullets and `✔`/`☐` checkboxes, blockquotes get a
  bar, links are underlined.
- **Syntax-highlighted code blocks** via [`syntect`](https://crates.io/crates/syntect).
- **Tables drawn as grids**, with column alignment; wide tables shrink and wrap
  their cells to fit the terminal.
- **Word wrapping that respects nesting** — wrapped list items stay indented and
  quote bars continue down every line. Resizing re-flows the document.
- **Live reload** — saves are detected with [`notify`](https://crates.io/crates/notify)
  and the view re-renders automatically.
- **Terminal UI** built on [`ratatui`](https://crates.io/crates/ratatui):
  scrollbar, status line, mouse-wheel scrolling.
- **Single dependency-light binary** — no browser, no server.

## Install

Prebuilt binaries for Linux, macOS, and Windows (x86_64 and ARM64 on Linux
and macOS) are attached to each [GitHub release](https://github.com/haynescd/lazymd/releases/latest).
The installer scripts download the right one and put `lazymd` on your `PATH`.

**Linux / macOS:**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/haynescd/lazymd/releases/latest/download/lazymd-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/haynescd/lazymd/releases/latest/download/lazymd-installer.ps1 | iex"
```

**From source** (Rust 1.88 or newer, via [rustup.rs](https://rustup.rs)):

```sh
cargo install --git https://github.com/haynescd/lazymd
```

or clone and build it yourself:

```sh
git clone https://github.com/haynescd/lazymd.git
cd lazymd
cargo build --release   # binary lands at target/release/lazymd
```

## Usage

```sh
lazymd path/to/notes.md
lazymd --help                  # flags, keys, and environment variables
lazymd --version
cargo run -- test/showcase.md  # during development
```

Open the same file in your editor, make a change, save — the preview updates.

| Key            | Action           |
| -------------- | ---------------- |
| `q` / `Esc` / `Ctrl-c` | Quit              |
| `j` / `↓`              | Scroll down       |
| `k` / `↑`              | Scroll up         |
| `d` / `Ctrl-d`         | Half page down    |
| `u` / `Ctrl-u`         | Half page up      |
| `Space` / `f` / `PgDn` | Page down         |
| `b` / `PgUp`           | Page up           |
| `g` / `Home`           | Jump to top       |
| `G` / `End`            | Jump to bottom    |
| `r`                    | Reload now        |
| mouse wheel            | Scroll            |

`test/showcase.md` exercises every element lazymd can draw.

## How it works

1. **Parse** — `comrak::parse_document` turns the Markdown into an AST (a tree of
   `NodeValue` enum variants).
2. **Render** — the `Renderer` struct walks that tree recursively. Inline nodes
   (bold, italic, code, links) fold into styled `ratatui` `Span`s; block nodes
   (headings, paragraphs, lists, code blocks, tables) word-wrap those spans to
   the terminal width and emit `Line`s. Each enclosing container (a list item,
   a blockquote) pushes a *prefix* — a bullet, a bar — that's re-applied to
   every line it produces.
3. **Highlight** — fenced code blocks go through `syntect`, whose RGB token
   colors map directly onto span styles.
4. **Watch** — `notify` watches the file's *parent directory* (more robust than
   the file itself, since many editors save by writing a temp file and renaming
   it) and sends a message over an `mpsc` channel.
5. **Loop** — the main loop draws the visible slice of lines, re-renders when
   the file changes or the width does, and polls for key and mouse input.

## Roadmap

Built as a learning ladder — each milestone runs on its own:

- [x] **M0–M2** — CLI arg, read file, parse to HTML with `comrak`
- [x] **M3** — walk the AST (enums, pattern matching, recursion, lifetimes)
- [x] **M4** — render the AST to styled text (structs, `impl`, `Vec`)
- [x] **M5** — TUI shell with `ratatui` (event loop, closures, RAII cleanup)
- [x] **M6** — live reload (channels, `move` closures)
- [x] **M7** — styled rendering (`Line`/`Span`), plus stretch goals:
  - [x] Syntax highlighting in code blocks (`syntect`)
  - [x] Table column alignment
  - [ ] Split editor / preview pane
  - [x] Unit tests for the renderer (`cargo test`)

## Notes

- Built against `comrak = "0.54"`; its AST enum field names shift between
  versions, so bumping it may require small changes in the render code.
- Code blocks use 24-bit color; terminals without truecolor support will show
  approximate colors.
- Link URLs aren't shown — just the underlined link text, as in a browser.
- Logs go to `$XDG_STATE_HOME/lazymd/lazymd.log` (usually
  `~/.local/state/lazymd/lazymd.log`; `%LOCALAPPDATA%\lazymd\lazymd.log` on
  Windows), overwritten on each launch. Pass `--log-level` (or set `LAZYMD_LOG`)
  to `off`, `error`, `warn`, `info` (the default), `debug`, or `trace` to change
  how much is written.

## License

MIT — see [LICENSE](LICENSE).
