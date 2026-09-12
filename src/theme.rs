//! The color scheme, in one place.
//!
//! Prose uses the terminal's named ANSI colors, so it follows whatever palette
//! the user's theme defines. Code is the exception: syntax highlighting produces
//! exact RGB, so code blocks (and inline code, to match) sit on the highlighter
//! theme's own background.

use ratatui::style::{Color, Modifier, Style};

/// Heading text, indexed by level - 1.
pub const HEADINGS: [Color; 6] = [
    Color::LightMagenta,
    Color::LightBlue,
    Color::LightCyan,
    Color::LightGreen,
    Color::LightYellow,
    Color::Gray,
];

/// Bullet glyphs and colors, cycled by list nesting depth.
pub const BULLETS: [(&str, Color); 3] = [
    ("•", Color::Cyan),
    ("◦", Color::LightBlue),
    ("▪", Color::Magenta),
];

/// Background shared by code blocks and inline code (base16-ocean.dark).
pub const CODE_BG: Color = Color::Rgb(0x2b, 0x30, 0x3b);
/// Foreground for inline code and for code blocks with no known language.
pub const CODE_FG: Color = Color::Rgb(0xeb, 0xcb, 0x8b);
/// syntect theme used for fenced code blocks.
pub const SYNTAX_THEME: &str = "base16-ocean.dark";

pub fn heading(level: u8) -> Style {
    let i = (level.clamp(1, 6) - 1) as usize;
    Style::new().fg(HEADINGS[i]).add_modifier(Modifier::BOLD)
}

/// The rule drawn under level 1 and 2 headings: the heading's color, dimmed.
pub fn heading_rule(level: u8) -> Style {
    heading(level)
        .remove_modifier(Modifier::BOLD)
        .add_modifier(Modifier::DIM)
}

pub fn bullet(depth: usize) -> (&'static str, Style) {
    let (glyph, color) = BULLETS[depth % BULLETS.len()];
    (glyph, Style::new().fg(color))
}

pub fn ordered_marker() -> Style {
    Style::new().fg(Color::Cyan)
}

pub fn task_done() -> Style {
    Style::new().fg(Color::Green)
}

pub fn task_todo() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Applied to the text of a checked task item.
pub fn task_done_text() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn inline_code() -> Style {
    Style::new().fg(CODE_FG).bg(CODE_BG)
}

pub fn code_block() -> Style {
    Style::new().fg(CODE_FG).bg(CODE_BG)
}

/// Just the background, for layering over syntax-highlighted spans — they
/// bring their own foreground.
pub fn code_bg() -> Style {
    Style::new().bg(CODE_BG)
}

pub fn code_label() -> Style {
    Style::new()
        .fg(Color::Rgb(0x65, 0x73, 0x7e))
        .bg(CODE_BG)
        .add_modifier(Modifier::ITALIC)
}

pub fn link() -> Style {
    Style::new()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::UNDERLINED)
}

pub fn image() -> Style {
    Style::new()
        .fg(Color::Magenta)
        .add_modifier(Modifier::ITALIC)
}

pub fn quote_bar() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn quote_text() -> Style {
    Style::new().fg(Color::Gray)
}

pub fn rule() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn table_border() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn table_header() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn html() -> Style {
    Style::new()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC)
}

pub fn footnote() -> Style {
    Style::new().fg(Color::Cyan)
}

pub fn alert(kind: comrak::nodes::AlertType) -> Style {
    use comrak::nodes::AlertType;
    let color = match kind {
        AlertType::Note => Color::LightBlue,
        AlertType::Tip => Color::LightGreen,
        AlertType::Important => Color::LightMagenta,
        AlertType::Warning => Color::LightYellow,
        AlertType::Caution => Color::LightRed,
    };
    Style::new().fg(color)
}

// --- TUI chrome ---

pub fn border() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn title() -> Style {
    Style::new()
        .fg(Color::LightMagenta)
        .add_modifier(Modifier::BOLD)
}

pub fn status() -> Style {
    Style::new().fg(Color::Gray)
}

pub fn status_key() -> Style {
    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

pub fn status_ok() -> Style {
    Style::new().fg(Color::Green)
}

pub fn status_err() -> Style {
    Style::new().fg(Color::LightRed)
}
