//! Syntax highlighting for fenced code blocks, via `syntect`.
//!
//! syntect's grammars and themes take a moment to deserialize, so they load
//! once on first use and are shared for the life of the program.

use std::sync::LazyLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::theme;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let mut themes = ThemeSet::load_defaults();
    themes
        .themes
        .remove(theme::SYNTAX_THEME)
        .expect("syntax theme is bundled with syntect")
});

/// Highlights `code` as `lang` (`rust`, `py`, `sh`, ...). One `Vec<Span>` per
/// source line, foreground only — the caller owns the background. `None` for an
/// unknown or missing language, so the caller can fall back to plain styling.
pub fn highlight(code: &str, lang: &str) -> Option<Vec<Vec<Span<'static>>>> {
    if lang.is_empty() {
        return None;
    }
    let syntax = SYNTAXES.find_syntax_by_token(lang)?;
    let mut highlighter = HighlightLines::new(syntax, &THEME);

    let mut lines = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, &SYNTAXES).ok()?;
        let spans = ranges
            .into_iter()
            .map(|(style, text)| {
                let text = text.trim_end_matches(['\n', '\r']).to_string();
                Span::styled(text, to_ratatui(style))
            })
            .filter(|span| !span.content.is_empty())
            .collect();
        lines.push(spans);
    }
    Some(lines)
}

fn to_ratatui(style: syntect::highlighting::Style) -> Style {
    let fg = style.foreground;
    let mut out = Style::new().fg(Color::Rgb(fg.r, fg.g, fg.b));
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_is_highlighted_in_several_colors() {
        let lines = highlight("fn main() {}\n", "rust").unwrap();
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "fn main() {}");
        let colors: std::collections::HashSet<_> = lines[0].iter().map(|s| s.style.fg).collect();
        assert!(colors.len() > 1, "expected more than one color");
    }

    #[test]
    fn unknown_language_falls_back() {
        assert!(highlight("x", "definitely-not-a-language").is_none());
        assert!(highlight("x", "").is_none());
    }
}
