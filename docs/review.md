# Code Review — render.rs, render/\*, wrap.rs

## Overview

codon is a Rust TUI markdown previewer. This review focuses on the rendering pipeline: the module that turns Markdown AST into styled terminal lines. The code is well-architected overall — the Canvas/Ctx/Render three-way state split, grapheme-aware wrapping, and comprehensive test suite are all standouts.

The issues below are ordered by severity.

---

## 1. Bugs & Correctness

### 1.1 `{r}` fence info string silently drops R highlighting

**File:** `render.rs`, `fence_language()`

```rust
fn fence_language(info: &str) -> &str {
    let end = info
        .find(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .unwrap_or(info.len());
    &info[..end]
}
```

`{r}` returns `""` because `{` is found at position 0, so R code blocks fall back to plain text. This is a comrak convention for R fenced code blocks.

**Fix:** Strip the leading `{`:

```rust
fn fence_language(info: &str) -> &str {
    let info = info.trim_start_matches('{');
    let end = info.find(|c: char| c.is_whitespace() || c == ',' || c == '}')
        .unwrap_or(info.len());
    &info[..end]
}
```

### 1.2 `chunk_by` requires Rust 1.89+ (edition 2024 minimum is 1.85)

**File:** `wrap.rs`, `wrap_words()` and `to_spans()`

`Slice::chunk_by` was stabilized in Rust 1.89.0. The project uses `edition = "2024"` (minimum Rust 1.85.0), so this fails to compile on Rust 1.85–1.88.

**Fix:** Polyfill it (~10 lines) or add a MSRV note to Cargo.toml.

### 1.3 Rule lines may overflow `avail` for multi-column tables

**File:** `render/table.rs`, `layout()`

```rust
const CELL_CHROME: usize = 3;
// ...
let budget = avail.saturating_sub(CELL_CHROME * columns + 1);
```

`CELL_CHROME = 3` (│ + space on each side) is correct for content lines, but the budget calculation overestimates available space. The actual border lines (`┌───┬───┐`) are `sum(widths) + 2*columns + 2` wide, not `sum(widths) + 3*columns + 1`. For tables with multiple columns, the top/bottom/middle rules can overflow by `columns - 1` columns.

**Fix:** Use `2 * columns + 2` for the budget:

```rust
let budget = avail.saturating_sub(2 * columns + 2);
```

### 1.4 `literal_block` silently drops trailing empty lines

**File:** `render.rs`, `literal_block()`

```rust
for line in literal.trim_end().lines() {
```

`trim_end()` strips trailing newlines before `lines()` is called, so a code block ending with blank lines loses them.

**Fix:** Use `lines()` without `trim_end()`, or `trim_end_matches('\n')` if you only want to strip the final newline.

---

## 2. Naming & Structure

### 2.1 `#[derive(Debug)]` on `Render` and `Canvas` — actively harmful

**File:** `render.rs`

```rust
#[derive(Debug)]
struct Render {
    out: Canvas,  // owns lines: Vec<Line<'static>>
    ctx: Ctx,
    footnotes_started: bool,
}

#[derive(Debug)]
struct Canvas {
    width: usize,
    lines: Vec<Line<'static>>,  // the entire rendered document
    prefixes: Vec<Prefix>,
    gap: Gap,
}
```

Both `Render` and `Canvas` own the output buffer. Debugging either of these prints the full rendered document:

```rust
dbg!(&render);  // prints every line rendered so far + prefixes + gap + Ctx
```

That's not debugging — that's dumping the document. The `Debug` impl here produces more noise than signal.

**Fix:** Remove `#[derive(Debug)]` from both. If you need to debug the rendering pipeline, debug `Ctx` and `Prefix` (which are small), not the output buffer.

### 2.2 `Canvas::rule` — noun-as-verb, leaks Markdown semantics

**File:** `render.rs`, `Canvas::rule()`

```rust
/// Emits a horizontal rule `width` columns wide.
fn rule(&mut self, glyph: &str, width: usize, style: Style) {
    self.push_line(vec![Span::styled(glyph.repeat(width), style)]);
}
```

`rule` describes a Markdown concept (thematic break), not a Canvas primitive. Canvas should be a dumb output layer — it shouldn't know about "rules." All three callers are doing the same operation: drawing a single row of repeated glyphs.

**Suggested fix:** Rename to `single_line` or inline the `push_line` call at each call site.

```rust
// Before
self.out.rule("─", self.out.avail(), theme::rule());

// After
self.out.single_line("─".repeat(self.out.avail()), theme::rule());
```

### 2.3 `Canvas::wrapped` — noun-as-verb, inconsistent with verb methods

**File:** `render.rs`, `Canvas::wrapped()`

Same pattern. `wrapped` describes the input, not the action. The action is "wrap and emit."

**Suggested fix:** `push_wrapped` or `render_wrapped`.

### 2.4 `Canvas::avail()` — noun, inconsistent with verb methods

**File:** `render.rs`, `Canvas::avail()`

`avail()` is a property accessor while `push_line`, `blank`, `wrapped` are verbs. This makes the API feel like two different designs were pasted together.

**Suggested fix:** `width_remaining()` or `available_width()`.

### 2.5 `Prefix::used` — reads as "was it consumed?" (opposite truth)

**File:** `render.rs`, `Prefix`

```rust
if !self.out.prefixes.last().is_some_and(|p| p.used) {
    self.out.push_line(Vec::new());
}
```

`used` is `true` when content *was* rendered, `false` when the container was empty. The name reads as "was this prefix consumed by the caller?" which is the opposite of the actual meaning.

**Suggested fix:** Rename to `rendered` or `content_emitted`.

### 2.6 `Gap` / `gap` — misnamed, should be `Spacing` / `spacing`

**File:** `render.rs`, `Gap` enum

`Gap` tracks whether a blank separator is needed before the next block, but the name doesn't convey that. The variants are worse:

| Variant | Reads as | Actual meaning |
|---------|----------|----------------|
| `Open` | "there's a gap open" | "nothing emitted yet" |
| `Blank` | "there's a gap" | "a blank line already exists" |
| `Content` | "there's content" | "next block needs a separator" |

The only meaningful question this enum answers is: **"Does the next block need a separator line?"**

**Suggested fix:**

```rust
enum Spacing {
    /// Nothing emitted yet; no separator needed.
    Fresh,
    /// A blank line was just emitted; no extra blank needed.
    Separated,
    /// Content was just emitted; a separator is needed.
    Content,
}
```

This makes `separate()` read almost self-documenting:

```rust
fn separate(&mut self) {
    if self.out.spacing == Spacing::Content && !self.ctx.tight {
        self.out.blank();
    }
}
```

### 2.7 `Render` — god struct, three concerns crammed into one

**File:** `render.rs`, `Render`

```rust
struct Render {
    out: Canvas,              // ← output buffer + layout state
    ctx: Ctx,                 // ← per-block context (style, tightness, depth)
    footnotes_started: bool,  // ← global document state
}
```

These three fields belong to different layers:

| Field | Scope | Mutability | Lifespan |
|-------|-------|------------|----------|
| `out` | Entire document | Append-only | Full render |
| `ctx` | Single block | Swapped in/out | One block at a time |
| `footnotes_started` | Entire document | Set once | Full render |

`ctx` is a scratchpad that's swapped via `scoped`. `out` is the output buffer. `footnotes_started` is a one-shot flag. They're all at the same struct level even though they have completely different roles.

### 2.8 `Render` name is misleading

`Render` reads like an action (what you do), but it's a container (what you hold). The struct is created, mutated, then its fields are extracted:

```rust
let mut render = Render::new(width);
render.block(root);
render.out.lines
```

The name "Render" obscures that this is a stateful session object.

**Suggested fix:** Rename to `RenderSession` or `Renderer`.

### 2.9 `Canvas` carries both output buffer and layout state

**File:** `render.rs`, `Canvas`

`Canvas` owns `lines` (output) but also `prefixes` and `gap` (layout state). The name "Canvas" suggests a dumb output buffer, but it's also the layout engine. `Render::out` doesn't tell you that "out" is not just a buffer — it's a full layout state machine.

**Suggested fix:** Split into `Output` (lines, push_line, blank) and `Layout` (width, prefixes, spacing).

---

## 3. Performance

### 3.1 `fit_columns` is O(sum(widths) − budget) per column

**File:** `render/table.rs`, `fit_columns()`

```rust
while widths.iter().sum::<usize>() > budget {
    let Some(widest) = widths.iter_mut().max() else { return };
    *widest -= 1;
}
```

For a table with columns `[100, 50, 50]` and budget `100`, this runs 100 iterations, each doing `O(n)` sum and `O(n)` max. A heap-based or binary-search approach would be O(n log n) worst case. Fine for typical markdown tables, but worth a TODO for very wide tables.

### 3.2 `rule` closure allocates `Vec<String>` on every call

**File:** `render/table.rs`, `draw()`

```rust
let rule = |left: &str, mid: &str, right: &str| {
    let segments: Vec<String> = grid.widths.iter().map(|w| "─".repeat(w + 2)).collect();
    // ...
};
```

Called 3 + separator_rows times per table. The segments only depend on `widths`, not the corner characters, so they could be hoisted out into a single allocation.

---

## 4. What's Done Well

| Area | Notes |
|------|-------|
| **Module docs** | `render.rs` top-level doc perfectly explains the Canvas/Ctx/Render split |
| **Gap enum** | Avoids contradictory boolean flags (no `has_content` + `has_blank`) |
| **Grapheme wrapping** | Heart emoji ❤️ and ZWJ sequences handled correctly |
| **Table 3-pass layout** | Render → Size → Draw cleanly separates concerns |
| **Prefix system** | `constant` vs `marker` variants handle blockquotes vs lists elegantly |
| **Test coverage** | Edge cases tested: emoji width, ZWJ, tight/loose lists, task dimming isolation |
| **`fence_language`** | Handles `python title="x"` and `js,twoslash` correctly (just not `{r}`) |
| **`Markers::at`** | Checkbox replaces bullet rather than sitting beside it — clean UX |
