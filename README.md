# Codon

A fast terminal Markdown previewer, written in Rust. Point it at a file and get
a live, styled render right in your terminal — edit in your editor of choice,
save, and the preview updates instantly.

> **Why "Codon"?** A codon is the smallest coding unit that gets *read and
> translated* into something else. That's the whole job of a previewer: read the
> Markdown source, render the output. (Started as a side project to learn Rust.)

## Features

- **GitHub Flavored Markdown** via [`comrak`](https://crates.io/crates/comrak):
  tables, task lists, strikethrough, and autolinks.
- **Live reload** — saves are detected with [`notify`](https://crates.io/crates/notify)
  and the view re-renders automatically.
- **Terminal UI** built on [`ratatui`](https://crates.io/crates/ratatui):
  scrollable, bordered, colored output.
- **Single dependency-light binary** — no browser, no server.

## Install

Requires a recent Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone <your-repo-url> codon
cd codon
cargo build --release
```

The binary lands at `target/release/codon`.

## Usage

```sh
cargo run -- sample.md        # during development
codon path/to/notes.md        # after `cargo install --path .`
```

Open the same file in your editor, make a change, save — the preview updates.

| Key            | Action           |
| -------------- | ---------------- |
| `q` / `Esc`    | Quit             |
| `j` / `↓`      | Scroll down      |
| `k` / `↑`      | Scroll up        |
| `d` / `PgDn`   | Page down        |
| `u` / `PgUp`   | Page up          |
| `g` / `Home`   | Jump to top      |

## How it works

1. **Parse** — `comrak::parse_document` turns the Markdown into an AST (a tree of
   `NodeValue` enum variants).
2. **Render** — the `Render` struct walks that tree recursively. Block nodes
   (headings, paragraphs, lists, code blocks, tables) become terminal lines;
   inline nodes (bold, italic, code, links) fold into styled `ratatui` spans.
3. **Watch** — `notify` watches the file's *parent directory* (more robust than
   the file itself, since many editors save by writing a temp file and renaming
   it) and sends a message over an `mpsc` channel.
4. **Loop** — the main loop draws the current lines, drains any pending change
   messages to re-render, and polls for key input.

## Roadmap

Built as a learning ladder — each milestone runs on its own:

- [x] **M0–M2** — CLI arg, read file, parse to HTML with `comrak`
- [x] **M3** — walk the AST (enums, pattern matching, recursion, lifetimes)
- [x] **M4** — render the AST to styled text (structs, `impl`, `Vec`)
- [x] **M5** — TUI shell with `ratatui` (event loop, closures, RAII cleanup)
- [x] **M6** — live reload (channels, `move` closures)
- [ ] **M7** — stretch goals:
  - [ ] Syntax highlighting in code blocks (`syntect`)
  - [ ] Table column alignment
  - [ ] Split editor / preview pane
  - [ ] Unit tests for the renderer

## Notes

- Pinned to `comrak = "0.24"`; its AST enum field names shift between versions,
  so bumping it may require small changes in the render code.
- Soft and hard line breaks are both rendered as spaces in v1.

## License

MIT (or your choice).
