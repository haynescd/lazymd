//! Wrapping styled text to a fixed width.
//!
//! ratatui's `Paragraph` can wrap on its own, but it knows nothing about our
//! prefixes: a wrapped blockquote line would lose its `▎` bar, and a wrapped
//! list item would snap back to column 0. So the renderer wraps each block
//! itself, to the width left over after its prefix, and re-applies the prefix
//! to every resulting line.
//!
//! Text is processed as `(grapheme, Style)` pairs. That's simpler than slicing
//! spans in place — a word can straddle several spans (`**bold**text`) — and is
//! plenty fast for documents a human wrote.
//!
//! Graphemes rather than `char`s, because a `char` is not a unit of width: the
//! two code points of `❤️` measure 1 column each but 2 together. Add up the
//! pieces and you get a width ratatui won't agree with when it draws the span,
//! which shows up as broken table borders.

use ratatui::{style::Style, text::Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One grapheme cluster and the style it is drawn in, borrowed from the span it
/// came from. `to_spans` copies it back out.
type StyledCluster<'a> = (&'a str, Style);

/// The only cluster that is a break opportunity.
const SPACE: &str = " ";
/// Tabs carry no width of their own, so prose and code alike expand them to
/// this before anything is measured.
const TAB: &str = "    ";

// --- measuring -------------------------------------------------------------

/// Display width of a run of spans — the measure ratatui uses to lay out a
/// `Line`, and the one everything below agrees with.
pub fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Display width of one grapheme cluster.
fn cluster_width(cluster: &str) -> usize {
    cluster.width()
}

/// Display width of a run of clusters.
fn clusters_width(clusters: &[StyledCluster]) -> usize {
    clusters.iter().map(|(g, _)| cluster_width(g)).sum()
}

/// Width of the widest line in `spans` when split only at `'\n'` — the room
/// the text would need to avoid wrapping.
pub fn max_line_width(spans: &[Span<'static>]) -> usize {
    hard_lines(spans)
        .iter()
        .map(|line| clusters_width(line))
        .max()
        .unwrap_or(0)
}

// --- wrapping --------------------------------------------------------------

/// Word-wraps `spans` to `width` columns. A `'\n'` inside the text forces a
/// break. Words longer than a whole line are split mid-word. Always returns at
/// least one (possibly empty) line.
pub fn wrap(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut out = Vec::new();
    for line in hard_lines(spans) {
        wrap_words(&line, width, &mut out);
    }
    out
}

/// Breaks `spans` at any cluster boundary, without regard for word boundaries —
/// for code, where whitespace is significant.
pub fn wrap_anywhere(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut out = Vec::new();
    for line in hard_lines(spans) {
        let mut buf = LineBuf::new(width);
        for cluster in line {
            buf.push(cluster, &mut out);
        }
        buf.finish(&mut out);
    }
    out
}

/// Right-pads `spans` with spaces in `style` until they are `to` columns wide.
pub fn pad(mut spans: Vec<Span<'static>>, to: usize, style: Style) -> Vec<Span<'static>> {
    let w = width(&spans);
    if w < to {
        spans.push(Span::styled(" ".repeat(to - w), style));
    }
    spans
}

/// Replaces tabs with spaces. `hard_lines` already does this for anything that
/// gets wrapped; call it directly before handing text to a syntax highlighter.
pub fn expand_tabs(s: &str) -> String {
    s.replace('\t', TAB)
}

// --- internals -------------------------------------------------------------

/// Flattens spans into styled clusters, split at every `'\n'`. Tabs become
/// spaces and other control characters are dropped, since they have no
/// sensible width.
fn hard_lines<'a>(spans: &'a [Span<'static>]) -> Vec<Vec<StyledCluster<'a>>> {
    let mut lines = vec![Vec::new()];
    for span in spans {
        for cluster in span.content.graphemes(true) {
            match cluster {
                "\n" | "\r\n" | "\r" => lines.push(Vec::new()),
                "\t" => {
                    let line = lines.last_mut().expect("starts with one line");
                    for _ in 0..TAB.len() {
                        line.push((SPACE, span.style));
                    }
                }
                c if c.chars().all(char::is_control) => {}
                c => lines
                    .last_mut()
                    .expect("starts with one line")
                    .push((c, span.style)),
            }
        }
    }
    lines
}

/// Collects clusters into a line, emitting it whenever it fills up. Both
/// wrapping strategies share it, so both break an over-long run the same way.
struct LineBuf<'a> {
    width: usize,
    cur: Vec<StyledCluster<'a>>,
    cur_width: usize,
}

impl<'a> LineBuf<'a> {
    fn new(width: usize) -> Self {
        LineBuf {
            width: width.max(1),
            cur: Vec::new(),
            cur_width: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.cur.is_empty()
    }

    /// Whether `extra` more columns would still fit on the current line.
    fn fits(&self, extra: usize) -> bool {
        self.cur_width + extra <= self.width
    }

    /// Appends a run already known to fit.
    fn extend(&mut self, clusters: &[StyledCluster<'a>]) {
        self.cur_width += clusters_width(clusters);
        self.cur.extend_from_slice(clusters);
    }

    /// Appends one cluster, breaking the line first if it no longer fits. A
    /// cluster too wide for any line still goes on one, and overflows.
    fn push(&mut self, cluster: StyledCluster<'a>, out: &mut Vec<Vec<Span<'static>>>) {
        let w = cluster_width(cluster.0);
        if !self.fits(w) && !self.is_empty() {
            self.emit(out);
        }
        self.cur_width += w;
        self.cur.push(cluster);
    }

    /// Ends the current line, if one has been started.
    fn line_break(&mut self, out: &mut Vec<Vec<Span<'static>>>) {
        if !self.is_empty() {
            self.emit(out);
        }
    }

    /// Ends the last line even when it is empty, so every hard line yields a row.
    fn finish(mut self, out: &mut Vec<Vec<Span<'static>>>) {
        self.emit(out);
    }

    fn emit(&mut self, out: &mut Vec<Vec<Span<'static>>>) {
        out.push(to_spans(&self.cur));
        self.cur.clear();
        self.cur_width = 0;
    }
}

/// Greedy word wrap of one hard line. Whitespace between words is kept when
/// the next word fits on the same line and dropped at a line break.
fn wrap_words<'a>(clusters: &[StyledCluster<'a>], width: usize, out: &mut Vec<Vec<Span<'static>>>) {
    let mut line = LineBuf::new(width);
    let mut gap: &[StyledCluster<'a>] = &[];

    // Only a plain space is a break opportunity — a non-breaking space stays
    // part of its word, which is what the author asked for.
    for token in clusters.chunk_by(|a, b| (a.0 == SPACE) == (b.0 == SPACE)) {
        if token[0].0 == SPACE {
            // Leading whitespace on a line is dropped; otherwise hold it until
            // we know whether the next word joins this line.
            if !line.is_empty() {
                gap = token;
            }
            continue;
        }

        if line.fits(clusters_width(gap) + clusters_width(token)) {
            line.extend(gap);
            line.extend(token);
            gap = &[];
            continue;
        }

        gap = &[];
        line.line_break(out);
        // The word starts a fresh line; split it if it's still too long.
        for &cluster in token {
            line.push(cluster, out);
        }
    }
    line.finish(out);
}

/// Re-joins styled clusters into spans, merging runs that share a style.
fn to_spans(clusters: &[StyledCluster]) -> Vec<Span<'static>> {
    clusters
        .chunk_by(|a, b| a.1 == b.1)
        .map(|run| Span::styled(run.iter().map(|(g, _)| *g).collect::<String>(), run[0].1))
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
    fn crlf_is_one_break_not_two() {
        let out = wrap(&[Span::raw("one\r\ntwo")], 80);
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
    fn wrap_anywhere_ignores_word_boundaries() {
        let out = wrap_anywhere(&[Span::raw("ab cd ef")], 3);
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

    #[test]
    fn tabs_expand_to_one_width_everywhere() {
        assert_eq!(expand_tabs("a\tb"), "a    b");
        // and hard_lines agrees, so un-expanded text measures the same
        let out = wrap(&[Span::raw("a\tb")], 80);
        assert_eq!(text(&out), ["a    b"]);
    }

    /// Wrapped lines must measure at most `width` by ratatui's own yardstick.
    #[test]
    fn emoji_clusters_measure_as_one_unit() {
        // U+2764 U+FE0F: 1 column per char, 2 as a cluster.
        let heart = "\u{2764}\u{fe0f}";
        assert_eq!(cluster_width(heart), 2);
        assert_eq!(max_line_width(&[Span::raw(heart.repeat(4))]), 8);

        for line in wrap(&[Span::raw(format!("{heart}{heart} {heart}{heart}"))], 6) {
            assert!(width(&line) <= 6, "{:?}", text(std::slice::from_ref(&line)));
        }
    }

    /// The other direction: a ZWJ sequence is 6 columns char-by-char, 2 as a cluster.
    #[test]
    fn zwj_sequences_do_not_wrap_early() {
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        assert_eq!(cluster_width(family), 2);
        // 2+1+2+1+2 = 8 columns, so they share a line at 8 and split at 7.
        let md = format!("{family} {family} {family}");
        let one = wrap(&[Span::raw(md.clone())], 8);
        assert_eq!(text(&one).concat(), md);
        assert_eq!(
            text(&wrap(&[Span::raw(md)], 7)),
            [format!("{family} {family}"), family.into()]
        );
    }

    #[test]
    fn a_cluster_is_never_split_in_half() {
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let out = wrap_anywhere(&[Span::raw(family.repeat(3))], 3);
        // 2 columns each, so one per line at width 3 — never a bare code point.
        assert_eq!(text(&out), [family, family, family]);
    }
}
