//! Turns Markdown into styled terminal lines.
//!
//! The AST walk is split the same way it always was: `block` handles the
//! vertical structure (paragraphs, lists, quotes, ...) and `inline` flattens a
//! block's contents into styled `Span`s. What's new is that output is built
//! from ratatui's rich-text types — a `Line` is a row of `Span`s, and each
//! `Span` carries its own `Style` — so a word can be bold without any `**`.
//!
//! Rendering happens at a fixed width, because wrapping has to happen here
//! (see `wrap.rs`) and rules and code blocks stretch to fill the line. The app
//! re-renders whenever the terminal is resized.

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

/// Narrowest width we'll wrap text to. If nesting eats more of the screen than
/// this, lines overflow and get clipped rather than collapsing to a letter per row.
const MIN_TEXT_WIDTH: usize = 10;
/// Narrowest a table column may be squeezed to.
const MIN_COLUMN_WIDTH: usize = 3;

/// Text drawn at the start of every line inside one enclosing container: a
/// blockquote's bar, a list item's marker. `first` goes on the container's
/// first line and `rest` on every line after. Both have the same width, so the
/// text they lead into stays aligned.
#[derive(Debug)]
struct Prefix {
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    used: bool,
}

impl Prefix {
    /// The same on every line, like a blockquote's bar.
    fn constant(spans: Vec<Span<'static>>) -> Self {
        Prefix {
            first: spans.clone(),
            rest: spans,
            used: false,
        }
    }

    /// Shown once, then replaced by blanks of the same width, like a bullet.
    fn marker(spans: Vec<Span<'static>>) -> Self {
        let width = wrap::width(&spans);
        Prefix {
            first: spans,
            rest: vec![Span::raw(" ".repeat(width))],
            used: false,
        }
    }
}

#[derive(Debug)]
struct Render {
    /// Total width available, in columns.
    width: usize,
    lines: Vec<Line<'static>>,
    /// One entry per container we're currently inside, outermost first.
    prefixes: Vec<Prefix>,
    /// Style every piece of text starts from — a blockquote greys it out, a
    /// checked task dims it.
    base: Style,
    /// Inside a tight list, blocks aren't separated by blank lines.
    tight: bool,
    list_depth: usize,
    last_blank: bool,
    /// A container just opened, so its first block starts right away rather
    /// than after a blank line.
    fresh: bool,
    footnotes_started: bool,
}

impl Render {
    pub fn new(width: usize) -> Self {
        Render {
            width,
            lines: Vec::new(),
            prefixes: Vec::new(),
            base: Style::default(),
            tight: false,
            list_depth: 0,
            last_blank: false,
            fresh: false,
            footnotes_started: false,
        }
    }

    // --- line output -------------------------------------------------------

    /// Width left for content once every active prefix has taken its share.
    fn avail(&self) -> usize {
        let used: usize = self.prefixes.iter().map(|p| wrap::width(&p.rest)).sum();
        self.width.saturating_sub(used).max(MIN_TEXT_WIDTH)
    }

    /// Emits one line of content behind the current prefixes.
    fn push_line(&mut self, content: Vec<Span<'static>>) {
        let mut spans = Vec::new();
        for p in &mut self.prefixes {
            if p.used {
                spans.extend(p.rest.iter().cloned());
            } else {
                spans.extend(p.first.iter().cloned());
                p.used = true;
            }
        }
        spans.extend(content);
        self.lines.push(Line::from(spans));
        self.last_blank = false;
        self.fresh = false;
    }

    /// Emits a blank separator line. It keeps visible prefixes (a quote's bar
    /// continues through it) but never uses up a pending list marker.
    fn blank(&mut self) {
        let spans: Vec<_> = self
            .prefixes
            .iter()
            .flat_map(|p| p.rest.iter().cloned())
            .collect();
        self.lines.push(Line::from(spans));
        self.last_blank = true;
    }

    /// Called at the start of each block: leaves one blank line between it and
    /// whatever came before, unless that would be wrong here.
    fn separate(&mut self) {
        if !self.lines.is_empty() && !self.last_blank && !self.tight && !self.fresh {
            self.blank();
        }
    }

    /// Word-wraps `spans` to the available width and emits the result.
    fn wrapped(&mut self, spans: Vec<Span<'static>>) {
        for line in wrap::wrap(&spans, self.avail()) {
            self.push_line(line);
        }
    }

    /// Runs `f` inside a container that contributes `prefix` to each line.
    fn nested(&mut self, prefix: Prefix, f: impl FnOnce(&mut Self)) {
        self.prefixes.push(prefix);
        self.fresh = true;
        f(self);
        // A container with no content (an empty list item) still shows its marker.
        if !self.prefixes.last().is_some_and(|p| p.used) {
            self.push_line(Vec::new());
        }
        self.prefixes.pop();
    }

    // --- blocks ------------------------------------------------------------

    fn children<'a>(&mut self, n: &'a AstNode<'a>) {
        for child in n.children() {
            self.block(child);
        }
    }

    fn block<'a>(&mut self, n: &'a AstNode<'a>) {
        let data = n.data.borrow();

        match &data.value {
            NodeValue::Document => self.children(n),
            NodeValue::Heading(h) => self.heading(n, h.level),
            NodeValue::Paragraph => {
                self.separate();
                let spans = self.inlines(n, self.base);
                self.wrapped(spans);
            }
            NodeValue::List(nl) => self.list(n, *nl),
            // Items are handled by `list`; these only appear if one is orphaned.
            NodeValue::Item(_) | NodeValue::TaskItem(_) => self.children(n),
            NodeValue::CodeBlock(cb) => self.code_block(&cb.info, &cb.literal),
            NodeValue::HtmlBlock(hb) => self.literal_block(&hb.literal),
            NodeValue::FrontMatter(fm) => self.literal_block(fm),
            NodeValue::ThematicBreak => {
                self.separate();
                let rule = "─".repeat(self.avail());
                self.push_line(vec![Span::styled(rule, theme::rule())]);
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                let bar = Span::styled("▎ ", theme::quote_bar());
                self.quote(n, bar, theme::quote_text(), None);
            }
            NodeValue::Alert(alert) => {
                let style = theme::alert(alert.alert_type);
                let title = alert
                    .title
                    .clone()
                    .unwrap_or_else(|| alert.alert_type.default_title().to_string());
                let title = Span::styled(title, style.add_modifier(Modifier::BOLD));
                self.quote(n, Span::styled("▎ ", style), Style::default(), Some(title));
            }
            NodeValue::Table(table) => self.table(n, &table.alignments),
            NodeValue::FootnoteDefinition(def) => {
                if !self.footnotes_started {
                    self.footnotes_started = true;
                    self.separate();
                    let rule = "─".repeat(self.avail().min(20));
                    self.push_line(vec![Span::styled(rule, theme::rule())]);
                }
                self.separate();
                let label = Span::styled(format!("[{}] ", def.name), theme::footnote());
                self.nested(Prefix::marker(vec![label]), |r| r.children(n));
            }
            _ => self.children(n),
        };
    }

    fn heading<'a>(&mut self, n: &'a AstNode<'a>, level: u8) {
        self.separate();
        let style = self.base.patch(theme::heading(level));
        let spans = self.inlines(n, style);
        self.wrapped(spans);

        // Like GitHub, the top two levels get a rule underneath.
        let underline = match level {
            1 => "━",
            2 => "─",
            _ => return,
        };
        let rule = underline.repeat(self.avail());
        self.push_line(vec![Span::styled(rule, theme::heading_rule(level))]);
    }

    fn list<'a>(&mut self, n: &'a AstNode<'a>, nl: NodeList) {
        self.separate();
        let saved_tight = std::mem::replace(&mut self.tight, nl.tight);
        let depth = self.list_depth;
        self.list_depth += 1;

        // Right-align numbers so `9.` and `10.` put their text in the same column.
        let last = nl.start + n.children().count().saturating_sub(1);
        let num_width = last.to_string().len();
        let delim = match nl.delimiter {
            ListDelimType::Period => '.',
            ListDelimType::Paren => ')',
        };

        for (i, item) in n.children().enumerate() {
            if i > 0 {
                self.separate(); // a no-op in tight lists
            }
            let checked = match item.data.borrow().value {
                NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                _ => None,
            };

            let mut marker = Vec::new();
            match nl.list_type {
                ListType::Ordered => marker.push(Span::styled(
                    format!("{:>num_width$}{delim} ", nl.start + i),
                    theme::ordered_marker(),
                )),
                // A checkbox replaces the bullet rather than sitting beside it.
                ListType::Bullet if checked.is_none() => {
                    let (glyph, style) = theme::bullet(depth);
                    marker.push(Span::styled(format!("{glyph} "), style));
                }
                ListType::Bullet => {}
            }
            match checked {
                Some(true) => marker.push(Span::styled("✔ ", theme::task_done())),
                Some(false) => marker.push(Span::styled("☐ ", theme::task_todo())),
                None => {}
            }

            let saved_base = self.base;
            if checked == Some(true) {
                self.base = self.base.patch(theme::task_done_text());
            }
            self.nested(Prefix::marker(marker), |r| r.children(item));
            self.base = saved_base;
        }

        self.list_depth = depth;
        self.tight = saved_tight;
    }

    /// A blockquote or alert: a colored bar down the left, an optional title.
    fn quote<'a>(
        &mut self,
        n: &'a AstNode<'a>,
        bar: Span<'static>,
        text: Style,
        title: Option<Span<'static>>,
    ) {
        self.separate();
        let saved_base = self.base;
        let saved_tight = std::mem::replace(&mut self.tight, false);
        self.base = self.base.patch(text);

        self.nested(Prefix::constant(vec![bar]), |r| {
            if let Some(title) = title {
                r.push_line(vec![title]);
                r.fresh = true; // no gap between an alert's title and its body
            }
            r.children(n);
        });

        self.base = saved_base;
        self.tight = saved_tight;
    }

    fn code_block(&mut self, info: &str, literal: &str) {
        self.separate();
        let lang = info
            .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
            .next()
            .unwrap_or("");
        let literal = literal.replace('\t', "    ");
        let bg = theme::code_block();

        let code: Vec<Vec<Span<'static>>> = match highlight::highlight(&literal, lang) {
            Some(lines) => lines
                .into_iter()
                .map(|l| {
                    l.into_iter()
                        .map(|s| s.patch_style(Style::new().bg(theme::CODE_BG)))
                        .collect()
                })
                .collect(),
            None => literal
                .lines()
                .map(|l| vec![Span::styled(l.to_string(), bg)])
                .collect(),
        };

        // Every row is padded to the full width, which is what turns a set of
        // lines into a solid block of background color.
        let width = self.avail();
        let label = if lang.is_empty() {
            String::new()
        } else {
            format!("{lang} ")
        };
        let label = Span::styled(label, theme::code_label());
        self.push_line(vec![
            Span::styled(" ".repeat(width.saturating_sub(label.width())), bg),
            label,
        ]);
        for line in code {
            for chunk in wrap::wrap_chars(&line, width.saturating_sub(2)) {
                let mut row = vec![Span::styled(" ", bg)];
                row.extend(chunk);
                self.push_line(wrap::pad(row, width, bg));
            }
        }
        self.push_line(wrap::pad(Vec::new(), width, bg));
    }

    /// Raw HTML or front matter: shown dimmed and verbatim. HTML comments are
    /// hidden, as they would be in a browser.
    fn literal_block(&mut self, literal: &str) {
        let trimmed = literal.trim();
        if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
            return;
        }
        self.separate();
        let width = self.avail();
        for line in literal.trim_end().lines() {
            let span = Span::styled(line.to_string(), self.base.patch(theme::html()));
            for chunk in wrap::wrap_chars(&[span], width) {
                self.push_line(chunk);
            }
        }
    }

    fn table<'a>(&mut self, n: &'a AstNode<'a>, alignments: &[TableAlignment]) {
        self.separate();

        // Pass 1: render every cell, since column widths depend on all rows.
        let mut rows: Vec<(bool, Vec<Vec<Span<'static>>>)> = Vec::new();
        for row in n.children() {
            let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
            let style = if header {
                self.base.patch(theme::table_header())
            } else {
                self.base
            };
            let cells = row
                .children()
                .map(|cell| self.inlines(cell, style))
                .collect();
            rows.push((header, cells));
        }
        let columns = rows.iter().map(|(_, cells)| cells.len()).max().unwrap_or(0);
        if columns == 0 {
            return;
        }

        let mut widths = vec![1; columns];
        for (_, cells) in &rows {
            for (w, cell) in widths.iter_mut().zip(cells) {
                *w = (*w).max(wrap::max_line_width(cell));
            }
        }
        // Each column costs its width plus `│ ` and a trailing space; one more
        // `│` closes the row.
        fit_columns(&mut widths, self.avail().saturating_sub(3 * columns + 1));

        // Pass 2: wrap each cell to its column and lay out the grid.
        let grid: Vec<(bool, Vec<Vec<Vec<Span<'static>>>>)> = rows
            .into_iter()
            .map(|(header, cells)| {
                let wrapped = (0..columns)
                    .map(|i| match cells.get(i) {
                        Some(cell) => wrap::wrap(cell, widths[i]),
                        None => vec![Vec::new()],
                    })
                    .collect();
                (header, wrapped)
            })
            .collect();
        // Once a cell spans several lines, rules between rows keep them readable.
        let multiline = grid
            .iter()
            .any(|(_, cells)| cells.iter().any(|c| c.len() > 1));

        let border = theme::table_border();
        let rule = |left: &str, mid: &str, right: &str| {
            let segments: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            vec![Span::styled(
                format!("{left}{}{right}", segments.join(mid)),
                border,
            )]
        };

        self.push_line(rule("┌", "┬", "┐"));
        for (r, (_, cells)) in grid.iter().enumerate() {
            if r > 0 && (grid[r - 1].0 || multiline) {
                self.push_line(rule("├", "┼", "┤"));
            }
            let height = cells.iter().map(Vec::len).max().unwrap_or(1);
            for k in 0..height {
                let mut line = vec![Span::styled("│", border)];
                for (i, cell) in cells.iter().enumerate() {
                    let content = cell.get(k).cloned().unwrap_or_default();
                    let align = alignments.get(i).copied().unwrap_or(TableAlignment::None);
                    line.push(Span::raw(" "));
                    line.extend(align_cell(content, widths[i], align));
                    line.push(Span::raw(" "));
                    line.push(Span::styled("│", border));
                }
                self.push_line(line);
            }
        }
        self.push_line(rule("└", "┴", "┘"));
    }

    // --- inlines -----------------------------------------------------------

    /// Renders all of `n`'s inline children, starting from `style`.
    fn inlines<'a>(&self, n: &'a AstNode<'a>, style: Style) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        self.inline_children(n, style, &mut out);
        out
    }

    fn inline_children<'a>(&self, n: &'a AstNode<'a>, style: Style, out: &mut Vec<Span<'static>>) {
        for child in n.children() {
            self.inline(child, style, out);
        }
    }

    /// Styles nest by patching: `**[a link](x)**` renders the link's text with
    /// the link style layered over bold.
    fn inline<'a>(&self, n: &'a AstNode<'a>, style: Style, out: &mut Vec<Span<'static>>) {
        let data = n.data.borrow();
        match &data.value {
            NodeValue::Text(t) => out.push(Span::styled(t.to_string(), style)),
            NodeValue::SoftBreak => out.push(Span::styled(" ", style)),
            NodeValue::LineBreak => out.push(Span::styled("\n", style)),
            NodeValue::Strong => self.inline_children(n, style.add_modifier(Modifier::BOLD), out),
            NodeValue::Emph => self.inline_children(n, style.add_modifier(Modifier::ITALIC), out),
            NodeValue::Strikethrough => {
                self.inline_children(n, style.add_modifier(Modifier::CROSSED_OUT), out)
            }
            NodeValue::Underline => {
                self.inline_children(n, style.add_modifier(Modifier::UNDERLINED), out)
            }
            NodeValue::Code(c) => out.push(Span::styled(
                c.literal.clone(),
                style.patch(theme::inline_code()),
            )),
            NodeValue::Math(m) => out.push(Span::styled(
                m.literal.clone(),
                style.patch(theme::inline_code()),
            )),
            NodeValue::Link(_) | NodeValue::WikiLink(_) => {
                self.inline_children(n, style.patch(theme::link()), out)
            }
            NodeValue::Image(_) => {
                let alt: String = self
                    .inlines(n, style)
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                let label = if alt.is_empty() {
                    "[image]".to_string()
                } else {
                    format!("[image: {alt}]")
                };
                out.push(Span::styled(label, style.patch(theme::image())));
            }
            NodeValue::HtmlInline(html) => {
                let tag = html.trim().to_ascii_lowercase();
                if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                    out.push(Span::styled("\n", style));
                } else if !tag.starts_with("<!--") {
                    out.push(Span::styled(html.clone(), style.patch(theme::html())));
                }
            }
            NodeValue::FootnoteReference(r) => out.push(Span::styled(
                format!("[{}]", r.name),
                style.patch(theme::footnote()),
            )),
            _ => self.inline_children(n, style, out),
        }
    }
}

/// Shrinks the widest columns, one column at a time, until the total fits in
/// `budget`. Stops early if every column is already at the minimum — the
/// table then overflows instead of becoming unreadable.
fn fit_columns(widths: &mut [usize], budget: usize) {
    while widths.iter().sum::<usize>() > budget {
        let Some(widest) = widths.iter_mut().max() else {
            return;
        };
        if *widest <= MIN_COLUMN_WIDTH {
            return;
        }
        *widest -= 1;
    }
}

/// Pads a cell's content out to `width` according to the column's alignment.
fn align_cell(
    content: Vec<Span<'static>>,
    width: usize,
    align: TableAlignment,
) -> Vec<Span<'static>> {
    let slack = width.saturating_sub(wrap::width(&content));
    let (left, right) = match align {
        TableAlignment::Right => (slack, 0),
        TableAlignment::Center => (slack / 2, slack - slack / 2),
        TableAlignment::Left | TableAlignment::None => (0, slack),
    };
    let mut out = Vec::with_capacity(content.len() + 2);
    if left > 0 {
        out.push(Span::raw(" ".repeat(left)));
    }
    out.extend(content);
    if right > 0 {
        out.push(Span::raw(" ".repeat(right)));
    }
    out
}

fn generate_options() -> Options<'static> {
    Options {
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
    }
}

/// Parses `md` and renders it to lines that fit in `width` columns.
pub fn render_ast(md: &str, width: usize) -> Vec<Line<'static>> {
    let options = generate_options();
    let arena = Arena::new();

    let root = parse_document(&arena, md, &options);

    let mut render = Render::new(width);
    render.block(root);

    render.lines
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
