# Learning Rust by Building Codon

This is a study guide, not documentation. Codon is structured as a ladder: each
milestone is a runnable program that adds a small, focused set of Rust concepts.
The finished code in `src/main.rs` is the top of the ladder — this doc is how you
climb it without skipping rungs.

## How to use this guide

1. **Read the mapped chapter first.** References are to *The Rust Programming
   Language* ("the Book"), free at <https://doc.rust-lang.org/book/>.
2. **Type it yourself.** Don't paste. Muscle memory and compiler errors are the
   point.
3. **Run it at every checkpoint.** Each milestone compiles and does something
   visible. If it doesn't run, don't move on.
4. **Diff against the reference only after yours works.** Comparing too early
   robs you of the struggle that teaches.
5. **Lean on the compiler.** Rust's error messages are unusually good. Read them
   fully before searching.

Fair pace: one milestone per sitting. M3 and M5 deserve two.

---

## M0 — Skeleton + CLI argument

**Goal:** `cargo run -- sample.md` prints the filename back to you.

**New concepts:** the cargo workflow, `fn main`, `env::args`, `Option`,
`Option::map`.

**Build:** `cargo new codon`. Read the first CLI argument with
`std::env::args().nth(1)`, which gives an `Option<String>`. Print it.

**Book:** ch. 1–3 (variables, types, functions, control flow).

**Checkpoint:** it echoes the path you pass. Passing nothing shouldn't crash yet
— just print a message.

**Hint if stuck:** `args()` yields the program name as item 0, so the file is
item 1 — that's why `.nth(1)`.

---

## M1 — Read the file

**Goal:** print the raw contents of the Markdown file.

**New concepts:** `Result`, the `?` operator, `String` vs `&str`, the first real
taste of ownership, then the `anyhow` crate for ergonomic errors.

**Build:** `std::fs::read_to_string(&path)` returns `Result<String, _>`. First
handle it with a `match`. Once that works, add `anyhow` and change `main` to
return `anyhow::Result<()>` so you can use `?`.

**Book:** ch. 4 (ownership), ch. 9 (error handling).

**Checkpoint:** the file's text dumps to your terminal; a missing file prints a
clean error instead of a panic.

**Hint if stuck:** `?` only works in a function that returns `Result` (or
`Option`). That's why `main`'s signature changes.

---

## M2 — Parse to HTML with comrak

**Goal:** print rendered HTML (`<h1>...</h1>`, etc.).

**New concepts:** adding a dependency, reading someone else's API, configuring a
struct's fields.

**Build:** add `comrak` to `Cargo.toml`. Use `comrak::markdown_to_html` with
`Options::default()`. Then turn on GFM extensions (`options.extension.table`,
`.strikethrough`, `.tasklist`, `.autolink`) and watch the output change.

**Book:** ch. 7 (packages & crates), ch. 5 (structs — for the options).

**Checkpoint:** valid HTML for your sample, tables included once you enable the
extension. This is your first "it actually does the thing" moment.

---

## M3 — Walk the AST *(the heart of Rust — go slow)*

**Goal:** print an indented outline of the document's structure instead of HTML.

**New concepts:** enums (`NodeValue`), `match` / pattern matching, recursion over
a tree, references and lifetimes (`&'a AstNode<'a>`), interior mutability
(`RefCell::borrow`).

**Build:** switch from `markdown_to_html` to `comrak::parse_document`, which
returns the AST root. Write a recursive function that, for each node, reads
`node.data.borrow().value`, prints its variant name with indentation, then
recurses into `node.children()`.

**Book:** ch. 6 (enums & matching), ch. 10 (generics, traits, **lifetimes**),
ch. 15 (`RefCell` and interior mutability).

**Checkpoint:** a readable tree — `Document → Heading → Text`, etc. — that mirrors
your Markdown.

**Hint if stuck:** the `'a` lifetime says "the nodes live as long as the arena
that owns them." You don't create lifetimes here so much as *thread the existing
one through*. Don't fight it; copy the signature shape and move on — it clicks
later.

---

## M4 — Render the AST to styled text

**Goal:** turn the tree into formatted lines (headings, bold, lists, code),
printed plainly or with basic ANSI color. No TUI yet.

**New concepts:** defining your own `struct` with `impl` methods, `&mut self`,
building up a `Vec`, separating *block* handling from *inline* handling.

**Build:** make a `Render` struct that owns a `Vec` of lines. Give it a method
that matches on block nodes and a second method that flattens inline nodes
(emphasis, strong, code) into styled pieces. This is the same split the finished
code uses.

**Book:** ch. 5 (structs & methods), ch. 8 (`Vec`, `String`).

**Checkpoint:** your sample looks *formatted* — headers stand out, list markers
appear, code is distinct — even as plain printed text.

---

## M5 — TUI shell with ratatui *(two sittings)*

**Goal:** show the rendered document in a scrollable, bordered terminal box.

**New concepts:** closures (the `draw(|f| ...)` callback), the event loop,
matching on key codes, and RAII cleanup — restoring the terminal even when the
program errors out.

**Build:** add `ratatui`. Wrap M4's lines in a `Paragraph` widget with a
`Block` border. Enter raw mode + the alternate screen, run a loop that draws and
reads key events, and handle `q`, arrows, and page keys. Crucially, restore the
terminal *after* the loop regardless of success or failure.

**Book:** ch. 13 (closures & iterators).

**Checkpoint:** a real TUI you scroll through and quit cleanly — no garbled
terminal left behind.

**Hint if stuck:** if a crash leaves your terminal broken, run `reset`. Then fix
the cleanup so it can't happen: compute the result, restore the terminal, *then*
return the result.

---

## M6 — Live reload

**Goal:** edit and save the file elsewhere; the preview updates on its own.

**New concepts:** channels (`mpsc`), `move` closures that capture ownership,
coalescing a burst of events into one refresh.

**Build:** add `notify`. Create an `mpsc::channel`; the watcher's callback (a
`move` closure that owns the sender) fires a message on any change. Each loop
iteration, drain all pending messages — if any arrived, re-read and re-render.
Watch the file's *parent directory*, not the file, so editor save-and-rename
still triggers it.

**Book:** ch. 16 (fearless concurrency — threads & channels).

**Checkpoint:** save in another window → preview refreshes. You've now rebuilt
the finished `src/main.rs` from scratch.

---

## M7 — Stretch goals (pick one)

Now you're past the guided path — these are open-ended, which is the real test.

- **Syntax highlighting** in code blocks with `syntect`. Teaches: working with a
  richer external API and mapping its output onto your styling.
- **Table column alignment** — measure the widest cell per column, pad the rest.
  Teaches: iterators in earnest, two-pass rendering.
- **Split editor / preview pane** — a much bigger jump; you're now writing a text
  editor. Teaches: layout, input modes, state management.
- **Unit tests** for the renderer with `#[test]`. Teaches: Rust's built-in test
  harness and designing code to be testable.

**Book:** ch. 11 (writing tests), ch. 13 (iterators) — and by now, the docs of
whatever crate you reach for.

---

## After the ladder

Once Codon feels understood, the concepts you haven't hit yet are: generics and
trait bounds in your own code (ch. 10), `Box`/`Rc` smart pointers (ch. 15), and
building your own iterators (ch. 13). A natural next side project is a small CLI
tool that reads a data file and reports on it — close to daily bioinformatics
work, and it exercises exactly those chapters.
