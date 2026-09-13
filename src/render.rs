//! Turns Markdown into styled terminal lines.
//!
//! `block` handles vertical structure (paragraphs, lists, quotes, ...) and
//! `inline` flattens a block's contents into styled `Span`s. A `Line` is a row
//! of `Span`s and each carries its own `Style`, so a word can be bold without
//! any `**`.
//!
//! Rendering happens at a fixed width, because wrapping happens here (see
//! `wrap.rs`) and rules and code blocks stretch to fill the line. The app
//! re-renders whenever the terminal is resized.
//!
//! State splits three ways: `Canvas` owns the output and how a line is placed,
//! `Ctx` is what a block inherits from the containers around it, passed down
//! the recursion by value, and `Renderer` holds the `Canvas` plus what's global
//! to the whole document. `Renderer` changes the `Canvas` only through its
//! methods, never by touching its fields.

use std::sync::LazyLock;

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListDelimType, ListType, NodeList, NodeValue, TableAlignment},
    options::Extension,
    parse_document,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::{highlight, theme, wrap};

mod inline;
mod table;

/// Narrowest width we'll wrap text to. If nesting eats more of the screen than
/// this, lines overflow and get clipped rather than collapsing to a letter per row.
const MIN_TEXT_WIDTH: usize = 10;
/// The footnote section's divider is short, not a full-width rule.
const FOOTNOTE_RULE_WIDTH: usize = 20;
/// Columns a code block spends on padding: one space either side of the code.
const CODE_GUTTER: usize = 2;

/// Text drawn at the start of every line inside one container: a blockquote's
/// bar, a list item's marker. `first` leads the container's first line, `rest`
/// every line after. The constructors keep the two the same width, which is
/// what keeps the text they lead into aligned.
#[derive(Debug)]
struct Prefix {
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    /// What this prefix costs on every line, whichever variant gets drawn.
    width: usize,
    /// Set once `first` has been drawn; every line after gets `rest`.
    used: bool,
}

impl Prefix {
    fn new(first: Vec<Span<'static>>, rest: Vec<Span<'static>>) -> Self {
        let width = wrap::width(&first);
        debug_assert_eq!(width, wrap::width(&rest), "prefix variants must align");
        Prefix {
            first,
            rest,
            width,
            used: false,
        }
    }

    /// The same on every line, like a blockquote's bar.
    fn constant(spans: Vec<Span<'static>>) -> Self {
        Self::new(spans.clone(), spans)
    }

    /// Shown once, then replaced by blanks of the same width, like a bullet.
    fn marker(spans: Vec<Span<'static>>) -> Self {
        let rest = vec![Span::raw(" ".repeat(wrap::width(&spans)))];
        Self::new(spans, rest)
    }

    /// The spans for the next line: the marker the first time, blanks after.
    fn take(&mut self) -> &[Span<'static>] {
        if std::mem::replace(&mut self.used, true) {
            &self.rest
        } else {
            &self.first
        }
    }
}

/// Everything an item's marker needs that is fixed for the whole list.
#[derive(Debug)]
struct Markers {
    list_type: ListType,
    start: usize,
    /// Width of the widest number, so `9.` and `10.` align their text.
    num_width: usize,
    delim: char,
    depth: usize,
}

impl Markers {
    fn new(nl: NodeList, count: usize, depth: usize) -> Self {
        let last = nl.start + count.saturating_sub(1);
        Markers {
            list_type: nl.list_type,
            start: nl.start,
            num_width: last.to_string().len(),
            delim: match nl.delimiter {
                ListDelimType::Period => '.',
                ListDelimType::Paren => ')',
            },
            depth,
        }
    }

    /// The marker for item `i`.
    fn at(&self, i: usize, checked: Option<bool>) -> Vec<Span<'static>> {
        let (num_width, delim) = (self.num_width, self.delim);
        let mut spans = Vec::new();
        match self.list_type {
            ListType::Ordered => spans.push(Span::styled(
                format!("{:>num_width$}{delim} ", self.start + i),
                theme::ordered_marker(),
            )),
            // A checkbox replaces the bullet rather than sitting beside it.
            ListType::Bullet if checked.is_none() => {
                let (glyph, style) = theme::bullet(self.depth);
                spans.push(Span::styled(format!("{glyph} "), style));
            }
            ListType::Bullet => {}
        }
        match checked {
            Some(true) => spans.push(Span::styled("✔ ", theme::task_done())),
            Some(false) => spans.push(Span::styled("☐ ", theme::task_todo())),
            None => {}
        }
        spans
    }
}

/// The rendered document, and everything needed to place one more line in it.
#[derive(Debug)]
struct Canvas {
    /// Total width available, in columns.
    width: usize,
    lines: Vec<Line<'static>>,
    /// One entry per container we're currently inside, outermost first.
    prefixes: Vec<Prefix>,
    /// Whether the next block needs a blank line before it: true after content,
    /// false at the start, after a blank line, and when a container opens.
    needs_separator: bool,
}

impl Canvas {
    fn new(width: usize) -> Self {
        Canvas {
            width,
            lines: Vec::new(),
            prefixes: Vec::new(),
            needs_separator: false,
        }
    }

    /// Width left for content once every active prefix has taken its share.
    fn content_width(&self) -> usize {
        let used: usize = self.prefixes.iter().map(|p| p.width).sum();
        self.width.saturating_sub(used).max(MIN_TEXT_WIDTH)
    }

    /// Enters a container that leads every line with `prefix`.
    fn open(&mut self, prefix: Prefix) {
        self.prefixes.push(prefix);
        self.needs_separator = false;
    }

    /// Leaves the innermost container.
    fn close(&mut self) {
        // A container with no content (an empty list item) still shows its marker.
        if !self.prefixes.last().is_some_and(|p| p.used) {
            self.push_line(Vec::new());
        }
        self.prefixes.pop();
        // It drew at least its own marker, so the next block needs a separator.
        self.needs_separator = true;
    }

    /// Emits one line of content behind the current prefixes.
    fn push_line(&mut self, content: Vec<Span<'static>>) {
        let mut spans = Vec::new();
        for p in &mut self.prefixes {
            spans.extend(p.take().iter().cloned());
        }
        spans.extend(content);
        self.lines.push(Line::from(spans));
        self.needs_separator = true;
    }

    /// Emits a line the next block follows directly, with no separator.
    fn push_title(&mut self, content: Vec<Span<'static>>) {
        self.push_line(content);
        self.needs_separator = false;
    }

    /// Emits a blank separator line. It keeps visible prefixes (a quote's bar
    /// continues through it) but never uses up a pending list marker.
    fn push_blank(&mut self) {
        let spans: Vec<_> = self
            .prefixes
            .iter()
            .flat_map(|p| p.rest.iter().cloned())
            .collect();
        self.lines.push(Line::from(spans));
        self.needs_separator = false;
    }

    /// Emits a blank line if content came before it; otherwise nothing.
    fn push_separator(&mut self) {
        if self.needs_separator {
            self.push_blank();
        }
    }

    /// Word-wraps `spans` to the content width and emits the result.
    fn push_wrapped(&mut self, spans: Vec<Span<'static>>) {
        for line in wrap::wrap(&spans, self.content_width()) {
            self.push_line(line);
        }
    }

    /// Emits a horizontal rule `width` columns wide.
    fn push_rule(&mut self, glyph: &str, width: usize, style: Style) {
        self.push_line(vec![Span::styled(glyph.repeat(width), style)]);
    }

    /// The finished document.
    fn into_lines(self) -> Vec<Line<'static>> {
        self.lines
    }
}

/// What a block inherits from the containers around it. Every block method
/// takes it by value, so a container changes it for its children only — the
/// call stack puts the caller's back when the container returns.
///
/// Prose inherits `base`; chrome doesn't. Borders, markers, rules and cell
/// padding are themed absolutely, so a table inside a blockquote keeps its own
/// border color.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    /// Style every piece of text starts from — a blockquote greys it out, a
    /// checked task dims it.
    base: Style,
    /// Inside a tight list, blocks aren't separated by blank lines.
    tight: bool,
    list_depth: usize,
}

#[derive(Debug)]
struct Renderer {
    out: Canvas,
    /// Footnote definitions share one divider, drawn before the first of them.
    footnotes_started: bool,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Renderer {
            out: Canvas::new(width),
            footnotes_started: false,
        }
    }

    // --- structure ---------------------------------------------------------

    /// Called at the start of each block: leaves one blank line between it and
    /// whatever came before, unless that would be wrong here.
    fn separate(&mut self, ctx: Ctx) {
        if !ctx.tight {
            self.out.push_separator();
        }
    }

    // --- blocks ------------------------------------------------------------

    fn blocks<'a>(&mut self, n: &'a AstNode<'a>, ctx: Ctx) {
        for child in n.children() {
            self.block(child, ctx);
        }
    }

    fn block<'a>(&mut self, n: &'a AstNode<'a>, ctx: Ctx) {
        let data = n.data.borrow();

        match &data.value {
            NodeValue::Document => self.blocks(n, ctx),
            NodeValue::Heading(h) => self.heading(n, h.level, ctx),
            NodeValue::Paragraph => {
                self.separate(ctx);
                let spans = inline::inlines(n, ctx.base);
                self.out.push_wrapped(spans);
            }
            NodeValue::List(nl) => self.list(n, *nl, ctx),
            // Items are handled by `list`; these only appear if one is orphaned.
            NodeValue::Item(_) | NodeValue::TaskItem(_) => self.blocks(n, ctx),
            NodeValue::CodeBlock(cb) => self.code_block(&cb.info, &cb.literal, ctx),
            NodeValue::HtmlBlock(hb) => self.literal_block(&hb.literal, ctx),
            NodeValue::FrontMatter(fm) => self.literal_block(fm, ctx),
            NodeValue::ThematicBreak => self.thematic_break(ctx),
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                let bar = Span::styled("▎ ", theme::quote_bar());
                self.quote(n, bar, theme::quote_text(), None, ctx);
            }
            NodeValue::Alert(alert) => {
                let style = theme::alert(alert.alert_type);
                let title = alert
                    .title
                    .clone()
                    .unwrap_or_else(|| alert.alert_type.default_title().to_string());
                let title = Span::styled(title, style.add_modifier(Modifier::BOLD));
                let bar = Span::styled("▎ ", style);
                self.quote(n, bar, Style::default(), Some(title), ctx);
            }
            NodeValue::Table(table) => self.table(n, &table.alignments, ctx),
            NodeValue::FootnoteDefinition(def) => self.footnote_definition(n, &def.name, ctx),
            _ => self.blocks(n, ctx),
        }
    }

    fn table<'a>(&mut self, n: &'a AstNode<'a>, alignments: &[TableAlignment], ctx: Ctx) {
        // Laid out before separating, so an empty table leaves no stray blank line.
        let width = self.out.content_width();
        let Some(rows) = table::render(n, ctx.base, width, alignments) else {
            return;
        };
        self.separate(ctx);
        for row in rows {
            self.out.push_line(row);
        }
    }

    fn heading<'a>(&mut self, n: &'a AstNode<'a>, level: u8, ctx: Ctx) {
        self.separate(ctx);
        let style = ctx.base.patch(theme::heading(level));
        let spans = inline::inlines(n, style);
        self.out.push_wrapped(spans);

        // Like GitHub, the top two levels get a rule underneath.
        let underline = match level {
            1 => "━",
            2 => "─",
            _ => return,
        };
        self.out.push_rule(
            underline,
            self.out.content_width(),
            theme::heading_rule(level),
        );
    }

    fn thematic_break(&mut self, ctx: Ctx) {
        self.separate(ctx);
        self.out
            .push_rule("─", self.out.content_width(), theme::rule());
    }

    /// Each item is a container led by its marker. `list` never draws the
    /// marker itself: the first line the item's content pushes picks it up (see
    /// `Prefix::take`), which is how `- - x` gets both bullets on one line.
    fn list<'a>(&mut self, n: &'a AstNode<'a>, nl: NodeList, ctx: Ctx) {
        self.separate(ctx);

        let markers = Markers::new(nl, n.children().count(), ctx.list_depth);
        let list_ctx = Ctx {
            tight: nl.tight,
            list_depth: ctx.list_depth + 1,
            ..ctx
        };

        for (i, item) in n.children().enumerate() {
            if i > 0 {
                self.separate(list_ctx); // a no-op in tight lists
            }
            let checked = match item.data.borrow().value {
                NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                _ => None,
            };

            let mut item_ctx = list_ctx;
            if checked == Some(true) {
                item_ctx.base = item_ctx.base.patch(theme::task_done_text());
            }
            self.out.open(Prefix::marker(markers.at(i, checked)));
            self.blocks(item, item_ctx);
            self.out.close();
        }
    }

    /// A blockquote or alert: a colored bar down the left, an optional title.
    fn quote<'a>(
        &mut self,
        n: &'a AstNode<'a>,
        bar: Span<'static>,
        text: Style,
        title: Option<Span<'static>>,
        ctx: Ctx,
    ) {
        self.separate(ctx);
        let quote_ctx = Ctx {
            tight: false,
            base: ctx.base.patch(text),
            ..ctx
        };

        self.out.open(Prefix::constant(vec![bar]));
        if let Some(title) = title {
            self.out.push_title(vec![title]);
        }
        self.blocks(n, quote_ctx);
        self.out.close();
    }

    fn footnote_definition<'a>(&mut self, n: &'a AstNode<'a>, name: &str, ctx: Ctx) {
        if !self.footnotes_started {
            self.footnotes_started = true;
            self.separate(ctx);
            let width = self.out.content_width().min(FOOTNOTE_RULE_WIDTH);
            self.out.push_rule("─", width, theme::rule());
        }
        self.separate(ctx);
        let label = Span::styled(format!("[{name}] "), theme::footnote());
        self.out.open(Prefix::marker(vec![label]));
        self.blocks(n, ctx);
        self.out.close();
    }

    fn code_block(&mut self, info: &str, literal: &str, ctx: Ctx) {
        self.separate(ctx);
        let lang = fence_language(info);
        // syntect reads indentation itself, so tabs are expanded before it sees them.
        let literal = wrap::expand_tabs(literal);
        let bg = theme::code_block();

        let code: Vec<Vec<Span<'static>>> = match highlight::highlight(&literal, lang) {
            Some(lines) => lines
                .into_iter()
                .map(|l| {
                    l.into_iter()
                        .map(|s| s.patch_style(theme::code_bg()))
                        .collect()
                })
                .collect(),
            None => literal
                .lines()
                .map(|l| vec![Span::styled(l.to_string(), bg)])
                .collect(),
        };

        // Padding every row to the full width is what makes the background solid.
        let width = self.out.content_width();

        // Right-aligned on the first line, and dropped rather than allowed to
        // push that one row wider than the rest.
        let label =
            (!lang.is_empty()).then(|| Span::styled(format!("{lang} "), theme::code_label()));
        let header = match label {
            Some(l) if l.width() <= width => {
                vec![Span::styled(" ".repeat(width - l.width()), bg), l]
            }
            _ => Vec::new(),
        };
        self.out.push_line(wrap::pad(header, width, bg));

        for line in code {
            for chunk in wrap::wrap_anywhere(&line, width.saturating_sub(CODE_GUTTER)) {
                let mut row = vec![Span::styled(" ", bg)];
                row.extend(chunk);
                self.out.push_line(wrap::pad(row, width, bg));
            }
        }
        self.out.push_line(wrap::pad(Vec::new(), width, bg));
    }

    /// Raw HTML or front matter: shown dimmed and verbatim. HTML comments are
    /// hidden, as they would be in a browser.
    fn literal_block(&mut self, literal: &str, ctx: Ctx) {
        let trimmed = literal.trim();
        if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
            return;
        }
        self.separate(ctx);
        let width = self.out.content_width();
        for line in literal.trim_end().lines() {
            let span = Span::styled(line.to_string(), ctx.base.patch(theme::html()));
            for chunk in wrap::wrap_anywhere(&[span], width) {
                self.out.push_line(chunk);
            }
        }
    }
}

/// The language from a fence's info string, which may carry more after it:
/// `rust`, `python title="x"`, `js,twoslash`, `{r}`.
fn fence_language(info: &str) -> &str {
    let end = info
        .find(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .unwrap_or(info.len());
    &info[..end]
}

/// Built once; `render_ast` runs on every resize.
static OPTIONS: LazyLock<Options<'static>> = LazyLock::new(|| Options {
    extension: Extension {
        table: true,
        strikethrough: true,
        tasklist: true,
        autolink: true,
        footnotes: true,
        alerts: true,
        front_matter_delimiter: Some("---".to_string()),
        ..Default::default()
    },
    ..Default::default()
});

/// Parses `md` and renders it to lines that fit in `width` columns.
pub fn render_ast(md: &str, width: usize) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let root = parse_document(&arena, md, &OPTIONS);

    let mut renderer = Renderer::new(width);
    renderer.block(root, Ctx::default());

    renderer.out.into_lines()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn render(md: &str) -> Vec<String> {
        plain(&render_ast(md, 40))
    }

    /// The first span whose text contains `needle`.
    fn span_with<'a>(lines: &'a [Line<'static>], needle: &str) -> &'a Span<'static> {
        lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("no span containing {needle:?}"))
    }

    #[test]
    fn heading_drops_hashes_and_is_bold() {
        let lines = render_ast("## Title", 40);
        assert_eq!(plain(&lines)[0], "Title");
        let span = span_with(&lines, "Title");
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(span.style.fg, Some(theme::HEADINGS[1]));
    }

    #[test]
    fn h1_and_h2_are_underlined_with_a_rule() {
        let out = render("# A\n\n### B");
        assert_eq!(out[1], "━".repeat(40));
        assert_eq!(out[3], "B");
    }

    #[test]
    fn emphasis_uses_modifiers_not_syntax() {
        let lines = render_ast("**bold** *it* ~~gone~~", 40);
        assert_eq!(plain(&lines), ["bold it gone"]);
        assert!(
            span_with(&lines, "bold")
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            span_with(&lines, "it")
                .style
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert!(
            span_with(&lines, "gone")
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
    }

    #[test]
    fn nested_emphasis_combines() {
        let lines = render_ast("***both***", 40);
        let m = span_with(&lines, "both").style.add_modifier;
        assert!(m.contains(Modifier::BOLD | Modifier::ITALIC));
    }

    #[test]
    fn inline_code_has_background() {
        let lines = render_ast("run `ls` now", 40);
        assert_eq!(plain(&lines), ["run ls now"]);
        assert_eq!(span_with(&lines, "ls").style.bg, Some(theme::CODE_BG));
    }

    #[test]
    fn link_shows_text_underlined() {
        let lines = render_ast("[docs](https://example.com)", 40);
        assert_eq!(plain(&lines), ["docs"]);
        let style = span_with(&lines, "docs").style;
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.fg, Some(Color::LightBlue));
    }

    #[test]
    fn paragraphs_are_separated_by_one_blank_line() {
        assert_eq!(render("one\n\ntwo"), ["one", "", "two"]);
    }

    #[test]
    fn long_paragraph_wraps() {
        let out = plain(&render_ast("aaa bbb ccc ddd", 10));
        assert_eq!(out, ["aaa bbb", "ccc ddd"]);
    }

    #[test]
    fn hard_break_starts_a_new_line() {
        assert_eq!(render("one  \ntwo"), ["one", "two"]);
    }

    #[test]
    fn tight_list_has_bullets_and_no_gaps() {
        assert_eq!(render("- a\n- b\n\nafter"), ["• a", "• b", "", "after"]);
    }

    #[test]
    fn loose_list_items_are_spaced() {
        assert_eq!(render("- a\n\n- b"), ["• a", "", "• b"]);
    }

    #[test]
    fn nested_list_indents_under_parent_text_with_a_new_glyph() {
        assert_eq!(render("- a\n  - b\n    - c"), ["• a", "  ◦ b", "    ▪ c"]);
    }

    #[test]
    fn ordered_list_right_aligns_numbers() {
        let md: String = (1..=10).map(|i| format!("{i}. x\n")).collect();
        let out = render(&md);
        assert_eq!(out[0], " 1. x");
        assert_eq!(out[9], "10. x");
    }

    #[test]
    fn wrapped_list_item_keeps_its_indent() {
        let out = plain(&render_ast("- aaa bbb ccc", 11));
        assert_eq!(out, ["• aaa bbb", "  ccc"]);
    }

    #[test]
    fn task_items_use_checkboxes_and_dim_when_done() {
        let lines = render_ast("- [x] done\n- [ ] todo", 40);
        assert_eq!(plain(&lines), ["✔ done", "☐ todo"]);
        assert_eq!(span_with(&lines, "done").style.fg, Some(Color::DarkGray));
        assert_eq!(span_with(&lines, "todo").style.fg, None);
    }

    /// A checked item dims only its own text — the next item starts clean.
    #[test]
    fn task_dimming_does_not_leak_to_the_next_item() {
        let lines = render_ast("- [x] done\n- plain", 40);
        assert_eq!(span_with(&lines, "plain").style.fg, None);
    }

    /// A quote's grey and its looseness end with the quote.
    #[test]
    fn quote_styling_does_not_leak_to_the_next_block() {
        let lines = render_ast("- > quoted\n- plain", 40);
        assert_eq!(span_with(&lines, "plain").style.fg, None);
        assert_eq!(plain(&lines), ["• ▎ quoted", "• plain"]);
    }

    #[test]
    fn blockquote_bar_continues_across_paragraphs_and_wraps() {
        let out = plain(&render_ast("> aaa bbb ccc\n>\n> ddd", 11));
        assert_eq!(out, ["▎ aaa bbb", "▎ ccc", "▎", "▎ ddd"]);
    }

    #[test]
    fn blockquote_inside_list_item() {
        let out = render("- item\n\n  > quoted");
        assert_eq!(out, ["• item", "", "  ▎ quoted"]);
    }

    #[test]
    fn code_block_inside_list_item_is_indented() {
        let out = render("- item\n\n  ```\n  code\n  ```");
        assert_eq!(out[0], "• item");
        assert!(out.iter().any(|l| l == "   code"), "{out:?}");
    }

    #[test]
    fn list_item_starting_with_a_nested_list_shares_the_line() {
        assert_eq!(render("- - inner"), ["• ◦ inner"]);
    }

    #[test]
    fn code_block_is_a_padded_block_with_a_label() {
        let lines = render_ast("```rust\nfn main() {}\n```", 30);
        let out = plain(&lines);
        assert_eq!(out[0].trim(), "rust");
        assert_eq!(out[1], " fn main() {}");
        // every row fills the width, so the background reads as a solid block
        for line in &lines {
            assert_eq!(line.width(), 30);
        }
        assert!(
            lines[1]
                .spans
                .iter()
                .all(|s| s.style.bg == Some(theme::CODE_BG))
        );
    }

    #[test]
    fn code_block_without_language_is_plain() {
        let out = render("```\nplain text\n```");
        assert_eq!(out, ["", " plain text", ""]);
    }

    /// A label too wide for the block is dropped rather than widening one row.
    #[test]
    fn code_block_label_never_widens_the_block() {
        let lines = render_ast("```averyveryverylonglanguagename\nx\n```", 12);
        for line in &lines {
            assert_eq!(line.width(), 12, "{:?}", plain(&lines));
        }
        assert_eq!(plain(&lines)[0], "");
    }

    #[test]
    fn fence_info_string_yields_just_the_language() {
        assert_eq!(fence_language("rust"), "rust");
        assert_eq!(fence_language("python title=\"x\""), "python");
        assert_eq!(fence_language("js,twoslash"), "js");
        assert_eq!(fence_language(""), "");
    }

    #[test]
    fn thematic_break_spans_the_width() {
        assert_eq!(render("a\n\n---\n\nb"), ["a", "", &"─".repeat(40), "", "b"]);
    }

    #[test]
    fn table_draws_a_grid_sized_to_its_content() {
        let out = render("| a | bb |\n|---|---:|\n| ccc | d |");
        assert_eq!(
            out,
            [
                "┌─────┬────┐",
                "│ a   │ bb │",
                "├─────┼────┤",
                "│ ccc │  d │",
                "└─────┴────┘",
            ]
        );
    }

    #[test]
    fn table_header_is_bold() {
        let lines = render_ast("| head |\n|---|\n| body |", 40);
        let bold = |t| {
            span_with(&lines, t)
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        };
        assert!(bold("head"));
        assert!(!bold("body"));
    }

    #[test]
    fn wide_table_shrinks_and_wraps_cells() {
        let md = "| a | b |\n|---|---|\n| one two three four | x |";
        let lines = render_ast(md, 16);
        for line in &lines {
            assert!(line.width() <= 16, "{:?}", plain(&lines));
        }
        let out = plain(&lines);
        assert!(out.len() > 5, "the long cell should wrap: {out:?}");
    }

    /// Columns are measured the same way the finished line is, so borders line up.
    #[test]
    fn table_rows_all_have_the_same_width() {
        let heart = "\u{2764}\u{fe0f}";
        let md = format!("| a | b |\n|---|---|\n| {heart}{heart}{heart} | y |\n| zzzz | w |");
        let lines = render_ast(&md, 40);
        let first = lines[0].width();
        for line in &lines {
            assert_eq!(line.width(), first, "{:?}", plain(&lines));
        }
    }

    #[test]
    fn html_is_shown_dimmed_but_comments_are_hidden() {
        let lines = render_ast("<div>hi</div>\n\n<!-- secret -->\n\nafter", 40);
        assert_eq!(plain(&lines), ["<div>hi</div>", "", "after"]);
        assert_eq!(span_with(&lines, "<div>").style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn inline_br_is_a_line_break() {
        assert_eq!(render("one<br>two"), ["one", "two"]);
    }

    #[test]
    fn alert_has_a_title_and_colored_bar() {
        let out = render("> [!WARNING]\n> Careful.");
        assert_eq!(out, ["▎ Warning", "▎ Careful."]);
    }

    #[test]
    fn footnotes_render_reference_and_definition() {
        let out = render("Text[^1].\n\n[^1]: The note.");
        assert_eq!(out[0], "Text[1].");
        assert!(out.iter().any(|l| l == "[1] The note."), "{out:?}");
    }

    #[test]
    fn image_shows_alt_text() {
        assert_eq!(render("![a cat](cat.png)"), ["[image: a cat]"]);
    }

    #[test]
    fn front_matter_is_dimmed_not_parsed_as_markdown() {
        let out = render("---\ntitle: x\n---\n\n# Hi");
        assert_eq!(out[..3], ["---", "title: x", "---"]);
        assert!(out.contains(&"Hi".to_string()));
    }

    #[test]
    fn empty_document_renders_nothing() {
        assert!(render_ast("", 40).is_empty());
    }
}
