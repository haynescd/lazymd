//! Flattening a block's inline children into styled spans.
//!
//! Nothing here reads renderer state: a run of inlines depends only on the node
//! and the style it inherits, so these are plain functions.

use comrak::nodes::{AstNode, NodeValue};
use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::theme;

/// Renders all of `n`'s inline children, starting from `style`.
pub(super) fn inlines<'a>(n: &'a AstNode<'a>, style: Style) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    inlines_into(n, style, &mut out);
    out
}

/// An image's alt text: the plain text of its children.
pub(super) fn alt_text<'a>(n: &'a AstNode<'a>) -> String {
    inlines(n, Style::default())
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

/// What an image shows when it can't be drawn: its alt text, marked as an image.
pub(super) fn image_label(alt: &str, style: Style) -> Span<'static> {
    let label = if alt.is_empty() {
        "[image]".to_string()
    } else {
        format!("[image: {alt}]")
    };
    Span::styled(label, style.patch(theme::image()))
}

fn inlines_into<'a>(n: &'a AstNode<'a>, style: Style, out: &mut Vec<Span<'static>>) {
    for child in n.children() {
        inline(child, style, out);
    }
}

/// Styles nest by patching: `**[a link](x)**` renders the link's text with
/// the link style layered over bold.
fn inline<'a>(n: &'a AstNode<'a>, style: Style, out: &mut Vec<Span<'static>>) {
    let data = n.data.borrow();
    match &data.value {
        NodeValue::Text(t) => out.push(Span::styled(t.to_string(), style)),
        NodeValue::SoftBreak => out.push(Span::styled(" ", style)),
        NodeValue::LineBreak => out.push(Span::styled("\n", style)),
        NodeValue::Strong => inlines_into(n, style.add_modifier(Modifier::BOLD), out),
        NodeValue::Emph => inlines_into(n, style.add_modifier(Modifier::ITALIC), out),
        NodeValue::Strikethrough => inlines_into(n, style.add_modifier(Modifier::CROSSED_OUT), out),
        NodeValue::Underline => inlines_into(n, style.add_modifier(Modifier::UNDERLINED), out),
        NodeValue::Code(c) => out.push(Span::styled(
            c.literal.clone(),
            style.patch(theme::inline_code()),
        )),
        NodeValue::Math(m) => out.push(Span::styled(
            m.literal.clone(),
            style.patch(theme::inline_code()),
        )),
        NodeValue::Link(_) | NodeValue::WikiLink(_) => {
            inlines_into(n, style.patch(theme::link()), out)
        }
        NodeValue::Image(_) => out.push(image_label(&alt_text(n), style)),
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
        _ => inlines_into(n, style, out),
    }
}
