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

use std::{path::Path, sync::LazyLock};

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

use crate::{render::image::ImageResolver, theme};

use image::ImageLoader;

mod highlight;
pub mod image;
mod inline;
mod table;
mod wrap;

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
    /// Set once the container has drawn a line; every line after gets `rest`.
    drawn: bool,
}

impl Prefix {
    fn new(first: Vec<Span<'static>>, rest: Vec<Span<'static>>) -> Self {
        let width = wrap::width(&first);
        debug_assert_eq!(width, wrap::width(&rest), "prefix variants must align");
        Prefix {
            first,
            rest,
            width,
            drawn: false,
        }
    }

    /// Shown on every line, like a blockquote's bar.
    fn every_line(spans: Vec<Span<'static>>) -> Self {
        Self::new(spans.clone(), spans)
    }

    /// Shown on the first line, like a bullet; later lines get blanks of the
    /// same width, so the text stays aligned.
    fn first_line(spans: Vec<Span<'static>>) -> Self {
        let rest = vec![Span::raw(" ".repeat(wrap::width(&spans)))];
        Self::new(spans, rest)
    }

    /// The spans to lead the next line with: `first` the first time, `rest` after.
    fn for_next_line(&mut self) -> &[Span<'static>] {
        if std::mem::replace(&mut self.drawn, true) {
            &self.rest
        } else {
            &self.first
        }
    }
}

/// A list item is plain, or a task that's still to do or done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Plain,
    Todo,
    Done,
}

impl ItemKind {
    fn of(value: &NodeValue) -> Self {
        match value {
            NodeValue::TaskItem(t) if t.symbol.is_some() => ItemKind::Done,
            NodeValue::TaskItem(_) => ItemKind::Todo,
            _ => ItemKind::Plain,
        }
    }

    /// The box a task is drawn with; plain items have none.
    fn checkbox(self) -> Option<Span<'static>> {
        match self {
            ItemKind::Plain => None,
            ItemKind::Todo => Some(Span::styled("☐ ", theme::task_todo())),
            ItemKind::Done => Some(Span::styled("✔ ", theme::task_done())),
        }
    }
}

/// What leads each item of one list, fixed for the whole list. As CommonMark
/// puts it, a list marker is a bullet list marker or an ordered list marker.
#[derive(Debug)]
enum ListMarker {
    /// The same glyph on every item.
    Bullet(Span<'static>),
    /// A number counting up from `start`.
    Ordered {
        start: usize,
        /// Width of the widest number, so `9.` and `10.` align their text.
        width: usize,
        delim: char,
    },
}

impl ListMarker {
    fn new(nl: NodeList, count: usize, depth: usize) -> Self {
        match nl.list_type {
            ListType::Bullet => {
                let (glyph, style) = theme::bullet(depth);
                ListMarker::Bullet(Span::styled(format!("{glyph} "), style))
            }
            ListType::Ordered => {
                let last = nl.start + count.saturating_sub(1);
                ListMarker::Ordered {
                    start: nl.start,
                    width: last.to_string().len(),
                    delim: match nl.delimiter {
                        ListDelimType::Period => '.',
                        ListDelimType::Paren => ')',
                    },
                }
            }
        }
    }

    /// The marker for the `i`th item (counting from 0).
    fn for_item(&self, i: usize, kind: ItemKind) -> Vec<Span<'static>> {
        match self {
            // A checkbox replaces the bullet rather than sitting beside it.
            ListMarker::Bullet(bullet) => vec![kind.checkbox().unwrap_or_else(|| bullet.clone())],
            // A number stays beside a checkbox: it says where the task sits.
            ListMarker::Ordered {
                start,
                width,
                delim,
            } => {
                let number = format!("{:>width$}{delim} ", start + i);
                let number = Span::styled(number, theme::ordered_marker());
                std::iter::once(number).chain(kind.checkbox()).collect()
            }
        }
    }
}

/// A rendered element: text lines or an image.
#[derive(Debug)]
pub enum RenderElement {
    /// A run of text rows.
    Lines(Vec<Line<'static>>),
    /// An image with its protocol and terminal height.
    Image(image::ImageDescriptor),
}

/// The rendered document, and everything needed to place one more line in it.
#[derive(Debug)]
struct Canvas {
    /// Total width available, in columns.
    width: usize,
    /// Text rows and images, in document order.
    elements: Vec<RenderElement>,
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
            elements: Vec::new(),
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
    fn open_container(&mut self, prefix: Prefix) {
        self.prefixes.push(prefix);
        self.needs_separator = false;
    }

    /// Leaves the innermost container.
    fn close_container(&mut self) {
        // A container with no content (an empty list item) still shows its marker.
        if !self.prefixes.last().is_some_and(|p| p.drawn) {
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
            spans.extend(p.for_next_line().iter().cloned());
        }
        spans.extend(content);
        self.push_row(Line::from(spans));
        self.needs_separator = true;
    }

    /// Adds a row to the trailing run of text, starting a new run after an image.
    fn push_row(&mut self, line: Line<'static>) {
        match self.elements.last_mut() {
            Some(RenderElement::Lines(lines)) => lines.push(line),
            _ => self.elements.push(RenderElement::Lines(vec![line])),
        }
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
        self.push_row(Line::from(spans));
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

    /// The finished document elements.
    fn into_elements(self) -> Vec<RenderElement> {
        self.elements
    }

    /// Emits an image. It sits outside the prefix system — the image renders
    /// into its own area, not behind any container prefix.
    fn push_image(&mut self, img: image::ImageDescriptor) {
        self.elements.push(RenderElement::Image(img));
        self.needs_separator = true;
    }
}

/// What a block inherits from the containers around it. Every block method
/// takes it by value, so a container changes it for its children only — the
/// call stack puts the caller's back when the container returns.
///
/// Prose inherits `base_style`; chrome doesn't. Borders, markers, rules and cell
/// padding are themed absolutely, so a table inside a blockquote keeps its own
/// border color.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    /// Style every piece of text starts from — a blockquote greys it out, a
    /// checked task dims it.
    base_style: Style,
    /// Inside a tight list, blocks aren't separated by blank lines.
    tight: bool,
    list_depth: usize,
}

#[derive(Debug)]
struct Renderer {
    canvas: Canvas,
    /// Footnote definitions share one divider, drawn before the first of them.
    footnotes_started: bool,
    /// Without a loader, images show as their alt text.
    images: ImageResolver,
}

impl Renderer {
    fn new(width: usize, md_filepath: String) -> Self {
        let images = ImageResolver::new(Path::new(&md_filepath));
        Renderer {
            canvas: Canvas::new(width),
            footnotes_started: false,
            images,
        }
    }

    // --- structure ---------------------------------------------------------

    /// Called at the start of each block: leaves one blank line between it and
    /// whatever came before, unless that would be wrong here.
    fn separate(&mut self, ctx: Ctx) {
        if !ctx.tight {
            self.canvas.push_separator();
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
            NodeValue::Paragraph => match sole_image(n) {
                Some((url, alt)) => self.image(&url, &alt, ctx),
                None => {
                    self.separate(ctx);
                    let spans = inline::inlines(n, ctx.base_style);
                    self.canvas.push_wrapped(spans);
                }
            },
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
        let width = self.canvas.content_width();
        let Some(rows) = table::render(n, ctx.base_style, width, alignments) else {
            return;
        };
        self.separate(ctx);
        for row in rows {
            self.canvas.push_line(row);
        }
    }

    fn heading<'a>(&mut self, n: &'a AstNode<'a>, level: u8, ctx: Ctx) {
        self.separate(ctx);
        let style = ctx.base_style.patch(theme::heading(level));
        let spans = inline::inlines(n, style);
        self.canvas.push_wrapped(spans);

        // Like GitHub, the top two levels get a rule underneath.
        let underline = match level {
            1 => "━",
            2 => "─",
            _ => return,
        };
        let width = self.canvas.content_width();
        self.canvas
            .push_rule(underline, width, theme::heading_rule(level));
    }

    fn thematic_break(&mut self, ctx: Ctx) {
        self.separate(ctx);
        let width = self.canvas.content_width();
        self.canvas.push_rule("─", width, theme::rule());
    }

    /// Each item is a container led by its marker. `list` never draws the
    /// marker itself: the first line the item's content pushes picks it up (see
    /// `Prefix::for_next_line`), which is how `- - x` gets both bullets on one line.
    fn list<'a>(&mut self, n: &'a AstNode<'a>, nl: NodeList, ctx: Ctx) {
        self.separate(ctx);

        let marker = ListMarker::new(nl, n.children().count(), ctx.list_depth);
        let list_ctx = Ctx {
            tight: nl.tight,
            list_depth: ctx.list_depth + 1,
            ..ctx
        };

        for (i, item) in n.children().enumerate() {
            if i > 0 {
                self.separate(list_ctx); // a no-op in tight lists
            }
            let kind = ItemKind::of(&item.data.borrow().value);

            let mut item_ctx = list_ctx;
            if kind == ItemKind::Done {
                item_ctx.base_style = item_ctx.base_style.patch(theme::task_done_text());
            }
            let prefix = Prefix::first_line(marker.for_item(i, kind));
            self.canvas.open_container(prefix);
            self.blocks(item, item_ctx);
            self.canvas.close_container();
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
            base_style: ctx.base_style.patch(text),
            ..ctx
        };

        self.canvas.open_container(Prefix::every_line(vec![bar]));
        if let Some(title) = title {
            self.canvas.push_title(vec![title]);
        }
        self.blocks(n, quote_ctx);
        self.canvas.close_container();
    }

    fn footnote_definition<'a>(&mut self, n: &'a AstNode<'a>, name: &str, ctx: Ctx) {
        if !self.footnotes_started {
            self.footnotes_started = true;
            self.separate(ctx);
            let width = self.canvas.content_width().min(FOOTNOTE_RULE_WIDTH);
            self.canvas.push_rule("─", width, theme::rule());
        }
        self.separate(ctx);
        let label = Span::styled(format!("[{name}] "), theme::footnote());
        self.canvas.open_container(Prefix::first_line(vec![label]));
        self.blocks(n, ctx);
        self.canvas.close_container();
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
        let width = self.canvas.content_width();

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
        self.canvas.push_line(wrap::pad(header, width, bg));

        for line in code {
            for chunk in wrap::wrap_anywhere(&line, width.saturating_sub(CODE_GUTTER)) {
                let mut row = vec![Span::styled(" ", bg)];
                row.extend(chunk);
                self.canvas.push_line(wrap::pad(row, width, bg));
            }
        }
        self.canvas.push_line(wrap::pad(Vec::new(), width, bg));
    }

    /// Raw HTML or front matter: shown dimmed and verbatim. HTML comments are
    /// hidden, as they would be in a browser.
    fn literal_block(&mut self, literal: &str, ctx: Ctx) {
        let trimmed = literal.trim();
        if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
            return;
        }
        self.separate(ctx);
        let width = self.canvas.content_width();
        for line in literal.trim_end().lines() {
            let span = Span::styled(line.to_string(), ctx.base_style.patch(theme::html()));
            for chunk in wrap::wrap_anywhere(&[span], width) {
                self.canvas.push_line(chunk);
            }
        }
    }

    /// An image on its own line: drawn if it loads, else its alt text.
    fn image(&mut self, url: &str, alt: &str, ctx: Ctx) {
        self.separate(ctx);
        let width = self.canvas.content_width();
        match self.images.resolve(url, width) {
            Some(image) => self.canvas.push_image(image),
            None => {
                log::warn!("couldn't load image {url:?}");
                self.canvas
                    .push_line(vec![inline::image_label(alt, ctx.base_style)]);
            }
        }
    }
}

/// A paragraph that is nothing but one image, as its url and alt text. Only
/// these become pictures: an image in a run of text stays as its alt text,
/// since a picture several rows tall can't sit inside a wrapped line.
fn sole_image<'a>(n: &'a AstNode<'a>) -> Option<(String, String)> {
    let child = n.first_child()?;
    if child.next_sibling().is_some() {
        return None;
    }
    match &child.data.borrow().value {
        NodeValue::Image(link) => Some((link.url.clone(), inline::alt_text(child))),
        _ => None,
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

/// Parses `md` and renders it to elements (text lines + images) that fit in
/// `width` columns. Without an image loader, images show as their alt text.
pub fn render_ast(md: &str, width: usize, images: Option<&ImageLoader>) -> Vec<RenderElement> {
    let md_path = md.to_string();
    let arena = Arena::new();
    let root = parse_document(&arena, md, &OPTIONS);

    let mut renderer = Renderer::new(width, md_path.to_string());
    renderer.block(root, Ctx::default());

    renderer.canvas.into_elements()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    /// The text rows of `md` rendered at `width`, with no image loader.
    fn render_lines(md: &str, width: usize) -> Vec<Line<'static>> {
        text_lines(render_ast(md, width, None))
    }

    /// Every text row, in order; images are skipped.
    fn text_lines(elements: Vec<RenderElement>) -> Vec<Line<'static>> {
        elements
            .into_iter()
            .flat_map(|e| match e {
                RenderElement::Lines(lines) => lines,
                RenderElement::Image(_) => Vec::new(),
            })
            .collect()
    }

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
        plain(&render_lines(md, 40))
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
        let lines = render_lines("## Title", 40);
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
        let lines = render_lines("**bold** *it* ~~gone~~", 40);
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
        let lines = render_lines("***both***", 40);
        let m = span_with(&lines, "both").style.add_modifier;
        assert!(m.contains(Modifier::BOLD | Modifier::ITALIC));
    }

    #[test]
    fn inline_code_has_background() {
        let lines = render_lines("run `ls` now", 40);
        assert_eq!(plain(&lines), ["run ls now"]);
        assert_eq!(span_with(&lines, "ls").style.bg, Some(theme::CODE_BG));
    }

    #[test]
    fn link_shows_text_underlined() {
        let lines = render_lines("[docs](https://example.com)", 40);
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
        let out = plain(&render_lines("aaa bbb ccc ddd", 10));
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
    fn ordered_list_keeps_its_start_and_delimiter() {
        let out = render("7) a\n8) b\n9) c\n10) d");
        assert_eq!(out, [" 7) a", " 8) b", " 9) c", "10) d"]);
    }

    /// Pinned to `theme::BULLETS` having three glyphs: the fourth level wraps.
    #[test]
    fn bullet_glyphs_cycle_with_depth() {
        let out = render("- a\n  - b\n    - c\n      - d");
        assert_eq!(out, ["• a", "  ◦ b", "    ▪ c", "      • d"]);
    }

    /// An empty item still shows its bullet: `close_container` pushes a line to carry it.
    #[test]
    fn empty_list_item_still_shows_its_marker() {
        assert_eq!(render("- a\n-\n- c"), ["• a", "•", "• c"]);
    }

    #[test]
    fn wrapped_list_item_keeps_its_indent() {
        let out = plain(&render_lines("- aaa bbb ccc", 11));
        assert_eq!(out, ["• aaa bbb", "  ccc"]);
    }

    #[test]
    fn task_items_use_checkboxes_and_dim_when_done() {
        let lines = render_lines("- [x] done\n- [ ] todo", 40);
        assert_eq!(plain(&lines), ["✔ done", "☐ todo"]);
        assert_eq!(span_with(&lines, "done").style.fg, Some(Color::DarkGray));
        assert_eq!(span_with(&lines, "todo").style.fg, None);
    }

    /// In an ordered list the checkbox sits beside the number, not in place of it.
    #[test]
    fn ordered_task_items_keep_their_number() {
        assert_eq!(
            render("1. [x] done\n2. [ ] todo"),
            ["1. ✔ done", "2. ☐ todo"]
        );
    }

    /// A checked item dims only its own text — the next item starts clean.
    #[test]
    fn task_dimming_does_not_leak_to_the_next_item() {
        let lines = render_lines("- [x] done\n- plain", 40);
        assert_eq!(span_with(&lines, "plain").style.fg, None);
    }

    /// A quote's grey and its looseness end with the quote.
    #[test]
    fn quote_styling_does_not_leak_to_the_next_block() {
        let lines = render_lines("- > quoted\n- plain", 40);
        assert_eq!(span_with(&lines, "plain").style.fg, None);
        assert_eq!(plain(&lines), ["• ▎ quoted", "• plain"]);
    }

    #[test]
    fn blockquote_bar_continues_across_paragraphs_and_wraps() {
        let out = plain(&render_lines("> aaa bbb ccc\n>\n> ddd", 11));
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
        let lines = render_lines("```rust\nfn main() {}\n```", 30);
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
        let lines = render_lines("```averyveryverylonglanguagename\nx\n```", 12);
        for line in &lines {
            assert_eq!(line.width(), 12, "{:?}", plain(&lines));
        }
        let out = plain(&lines);
        assert_eq!(out[0], "");
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
        let lines = render_lines("| head |\n|---|\n| body |", 40);
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
        let lines = render_lines(md, 16);
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
        let lines = render_lines(&md, 40);
        let first = lines[0].width();
        for line in &lines {
            assert_eq!(line.width(), first, "{:?}", plain(&lines));
        }
    }

    #[test]
    fn html_is_shown_dimmed_but_comments_are_hidden() {
        let lines = render_lines("<div>hi</div>\n\n<!-- secret -->\n\nafter", 40);
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

    /// A loader that can't find the file falls back to the alt text, not the url.
    #[test]
    fn missing_image_falls_back_to_alt_text() {
        let loader = ImageLoader::new(
            ratatui_image::picker::Picker::halfblocks(),
            std::path::Path::new("no/such/dir/doc.md"),
        );
        let out = text_lines(render_ast("![a cat](cat.png)", 40, Some(&loader)));
        assert_eq!(plain(&out), ["[image: a cat]"]);
    }

    #[test]
    fn image_inside_text_stays_inline() {
        assert_eq!(
            render("see ![a cat](cat.png) here"),
            ["see [image: a cat] here"]
        );
    }

    #[test]
    fn front_matter_is_dimmed_not_parsed_as_markdown() {
        let out = render("---\ntitle: x\n---\n\n# Hi");
        assert_eq!(out[..3], ["---", "title: x", "---"]);
        assert!(out.contains(&"Hi".to_string()));
    }

    #[test]
    fn empty_document_renders_nothing() {
        assert!(render_lines("", 40).is_empty());
    }
}
