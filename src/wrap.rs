//! Wrapping styled text to a fixed width.
//!
//! ratatui's `Paragraph` can wrap on its own, but it knows nothing about our
//! prefixes: a wrapped blockquote line would lose its `▎` bar, and a wrapped
//! list item would snap back to column 0. So the renderer wraps each block
//! itself, to the width left over after its prefix, and re-applies the prefix
//! to every resulting line.
//!
//! Text is processed as `(char, Style)` pairs. That's simpler than slicing
//! spans in place — a word can straddle several spans (`**bold**text`) — and
//! is plenty fast for documents a human wrote.

use ratatui::{style::Style, text::Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

type StyledChar = (char, Style);

/// Display width of a run of spans, in terminal columns.
pub fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Width of the widest line in `spans` when split only at `'\n'` — the room
/// the text would need to avoid wrapping.
pub fn max_line_width(spans: &[Span<'static>]) -> usize {
    hard_lines(spans)
        .iter()
        .map(|line| line.iter().map(|(c, _)| c.width().unwrap_or(0)).sum())
        .max()
        .unwrap_or(0)
}

/// Word-wraps `spans` to `width` columns. A `'\n'` inside the text forces a
/// break. Words longer than a whole line are split mid-word. Always returns at
/// least one (possibly empty) line.
pub fn wrap(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in hard_lines(spans) {
        wrap_words(&line, width, &mut out);
    }
    out
}

/// Breaks `spans` into chunks of at most `width` columns without regard for
/// word boundaries — for code, where whitespace is significant.
pub fn wrap_chars(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in hard_lines(spans) {
        let mut cur = Vec::new();
        let mut cur_w = 0;
        for (c, s) in line {
            let cw = c.width().unwrap_or(0);
            if cur_w + cw > width && !cur.is_empty() {
                out.push(to_spans(&cur));
                cur.clear();
                cur_w = 0;
            }
            cur.push((c, s));
            cur_w += cw;
        }
        out.push(to_spans(&cur));
    }
    out
}

/// Right-pads `spans` with spaces in `style` until they are `width` columns wide.
pub fn pad(mut spans: Vec<Span<'static>>, width: usize, style: Style) -> Vec<Span<'static>> {
    let w = self::width(&spans);
    if w < width {
        spans.push(Span::styled(" ".repeat(width - w), style));
    }
    spans
}

/// Flattens spans into styled chars, split at every `'\n'`. Tabs become spaces
/// and other control characters are dropped, since they have no sensible width.
fn hard_lines(spans: &[Span<'static>]) -> Vec<Vec<StyledChar>> {
    let mut lines = vec![Vec::new()];
    for span in spans {
        for c in span.content.chars() {
            match c {
                '\n' => lines.push(Vec::new()),
                '\t' => lines.last_mut().unwrap().push((' ', span.style)),
                c if c.is_control() => {}
                c => lines.last_mut().unwrap().push((c, span.style)),
            }
        }
    }
    lines
}

/// Greedy word wrap of one hard line. Whitespace between words is kept when
/// the next word fits on the same line and dropped at a line break.
fn wrap_words(chars: &[StyledChar], width: usize, out: &mut Vec<Vec<Span<'static>>>) {
    let mut line: Vec<StyledChar> = Vec::new();
    let mut line_w = 0;
    let mut gap: &[StyledChar] = &[];

    // Only a plain space is a break opportunity — a non-breaking space stays
    // part of its word, which is what the author asked for.
    for token in chars.chunk_by(|a, b| (a.0 == ' ') == (b.0 == ' ')) {
        if token[0].0 == ' ' {
            // Leading whitespace on a line is dropped; otherwise hold it until
            // we know whether the next word joins this line.
            if !line.is_empty() {
                gap = token;
            }
            continue;
        }

        let word_w: usize = token.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
        let gap_w = gap.len(); // spaces are 1 column each
        if line_w + gap_w + word_w <= width {
            line.extend_from_slice(gap);
            line.extend_from_slice(token);
            line_w += gap_w + word_w;
            gap = &[];
            continue;
        }

        gap = &[];
        if !line.is_empty() {
            out.push(to_spans(&line));
            line.clear();
            line_w = 0;
        }
        // The word starts a fresh line; split it if it's still too long.
        for &(c, s) in token {
            let cw = c.width().unwrap_or(0);
            if line_w + cw > width && !line.is_empty() {
                out.push(to_spans(&line));
                line.clear();
                line_w = 0;
            }
            line.push((c, s));
            line_w += cw;
        }
    }
    out.push(to_spans(&line));
}

/// Re-joins styled chars into spans, merging runs that share a style.
fn to_spans(chars: &[StyledChar]) -> Vec<Span<'static>> {
    chars
        .chunk_by(|a, b| a.1 == b.1)
        .map(|run| Span::styled(run.iter().map(|(c, _)| c).collect::<String>(), run[0].1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Modifier, Style};

    fn text(lines: &[Vec<Span>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn short_text_is_one_line() {
        let out = wrap(&[Span::raw("hello world")], 20);
        assert_eq!(text(&out), ["hello world"]);
    }

    #[test]
    fn wraps_at_word_boundaries_and_drops_the_gap() {
        let out = wrap(&[Span::raw("the quick brown fox")], 10);
        assert_eq!(text(&out), ["the quick", "brown fox"]);
    }

    #[test]
    fn splits_words_longer_than_the_line() {
        let out = wrap(&[Span::raw("abcdefghij")], 4);
        assert_eq!(text(&out), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn newline_forces_a_break() {
        let out = wrap(&[Span::raw("one\ntwo")], 80);
        assert_eq!(text(&out), ["one", "two"]);
    }

    #[test]
    fn keeps_styles_across_a_word_that_spans_several_spans() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let out = wrap(
            &[
                Span::raw("aaaa "),
                Span::styled("bb", bold),
                Span::raw("cc"),
            ],
            5,
        );
        assert_eq!(text(&out), ["aaaa", "bbcc"]);
        assert_eq!(out[1][0].style, bold);
        assert_eq!(out[1][1].style, Style::default());
    }

    #[test]
    fn wrap_chars_ignores_word_boundaries() {
        let out = wrap_chars(&[Span::raw("ab cd ef")], 3);
        assert_eq!(text(&out), ["ab ", "cd ", "ef"]);
    }

    #[test]
    fn pad_fills_to_width() {
        let out = pad(vec![Span::raw("ab")], 5, Style::default());
        assert_eq!(width(&out), 5);
    }

    #[test]
    fn wide_chars_count_as_two_columns() {
        let out = wrap(&[Span::raw("日本 語")], 4);
        assert_eq!(text(&out), ["日本", "語"]);
    }
}
