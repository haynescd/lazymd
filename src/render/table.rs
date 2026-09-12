//! Laying out and drawing a GFM table.
//!
//! Column widths depend on every row, so the whole table is rendered and
//! measured before a single line of it is drawn. Nothing here touches the
//! renderer's state: it takes the inherited style and the width it has to fit
//! in, and hands back finished rows.

use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use ratatui::{style::Style, text::Span};

use super::inline;
use crate::{theme, wrap};

/// Narrowest a column may be squeezed to.
const MIN_COLUMN_WIDTH: usize = 3;
/// Columns a cell spends on chrome: its opening `│` and a space either side.
/// `layout` budgets for it and `draw` draws it — keep the two in step.
const CELL_CHROME: usize = 3;

/// One table cell's content, wrapped to its column: one entry per line.
type CellLines = Vec<Vec<Span<'static>>>;

#[derive(Debug)]
struct Row {
    header: bool,
    /// One entry per column, even where the source row was short.
    cells: Vec<CellLines>,
}

/// A row's cells as rendered, before column widths are known.
#[derive(Debug)]
struct RawRow {
    header: bool,
    cells: Vec<Vec<Span<'static>>>,
}

/// A table laid out: every cell wrapped to a column width that fits the screen.
#[derive(Debug)]
struct Grid {
    rows: Vec<Row>,
    widths: Vec<usize>,
    /// Whether any cell wrapped — if so, rules between rows keep them readable.
    multiline: bool,
}

/// Lays `n` out to fit in `avail` columns and draws it. `None` if it has no
/// columns at all.
pub(super) fn render<'a>(
    n: &'a AstNode<'a>,
    base: Style,
    avail: usize,
    alignments: &[TableAlignment],
) -> Option<Vec<Vec<Span<'static>>>> {
    Some(draw(&layout(n, base, avail)?, alignments))
}

/// Renders every cell, sizes the columns to fit, and wraps each cell to its
/// column. `None` if the table has no columns.
fn layout<'a>(n: &'a AstNode<'a>, base: Style, avail: usize) -> Option<Grid> {
    // Pass 1: render every cell, since column widths depend on all rows.
    let rendered: Vec<RawRow> = n
        .children()
        .map(|row| {
            let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
            let style = if header {
                base.patch(theme::table_header())
            } else {
                base
            };
            let cells = row
                .children()
                .map(|cell| inline::inlines(cell, style))
                .collect();
            RawRow { header, cells }
        })
        .collect();

    let columns = rendered
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return None;
    }

    // Pass 2: give each column its widest cell, then shrink to fit.
    let mut widths = vec![1; columns];
    for row in &rendered {
        for (w, cell) in widths.iter_mut().zip(&row.cells) {
            *w = (*w).max(wrap::max_line_width(cell));
        }
    }
    let budget = avail.saturating_sub(CELL_CHROME * columns + 1);
    fit_columns(&mut widths, budget);

    // Pass 3: wrap each cell to the column it ended up with.
    let rows: Vec<Row> = rendered
        .into_iter()
        .map(|raw| Row {
            header: raw.header,
            cells: (0..columns)
                .map(|i| match raw.cells.get(i) {
                    Some(cell) => wrap::wrap(cell, widths[i]),
                    None => vec![Vec::new()],
                })
                .collect(),
        })
        .collect();

    let multiline = rows.iter().any(|row| row.cells.iter().any(|c| c.len() > 1));

    Some(Grid {
        rows,
        widths,
        multiline,
    })
}

fn draw(grid: &Grid, alignments: &[TableAlignment]) -> Vec<Vec<Span<'static>>> {
    let mut out = Vec::new();
    let border = theme::table_border();
    let rule = |left: &str, mid: &str, right: &str| {
        let segments: Vec<String> = grid.widths.iter().map(|w| "─".repeat(w + 2)).collect();
        vec![Span::styled(
            format!("{left}{}{right}", segments.join(mid)),
            border,
        )]
    };

    out.push(rule("┌", "┬", "┐"));
    for (r, row) in grid.rows.iter().enumerate() {
        if r > 0 && (grid.rows[r - 1].header || grid.multiline) {
            out.push(rule("├", "┼", "┤"));
        }
        let height = row.cells.iter().map(Vec::len).max().unwrap_or(1);
        for k in 0..height {
            let mut line = vec![Span::styled("│", border)];
            for (i, cell) in row.cells.iter().enumerate() {
                let content = cell.get(k).cloned().unwrap_or_default();
                let align = alignments.get(i).copied().unwrap_or(TableAlignment::None);
                line.push(Span::raw(" "));
                line.extend(align_cell(content, grid.widths[i], align));
                line.push(Span::raw(" "));
                line.push(Span::styled("│", border));
            }
            out.push(line);
        }
    }
    out.push(rule("└", "┴", "┘"));
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_columns_shrinks_the_widest_first() {
        let mut w = vec![10, 4, 4];
        fit_columns(&mut w, 16);
        assert_eq!(w, [8, 4, 4]);
    }

    #[test]
    fn fit_columns_levels_columns_it_has_to_shrink() {
        let mut w = vec![20, 20, 4];
        fit_columns(&mut w, 24);
        assert_eq!(w.iter().sum::<usize>(), 24);
        assert_eq!(
            w[0].abs_diff(w[1]),
            0,
            "the two wide columns end up even: {w:?}"
        );
    }

    /// A table too wide even at the minimum overflows rather than becoming
    /// unreadable, so the loop has to give up instead of spinning.
    #[test]
    fn fit_columns_stops_at_the_minimum() {
        let mut w = vec![9, 9];
        fit_columns(&mut w, 1);
        assert_eq!(w, [MIN_COLUMN_WIDTH, MIN_COLUMN_WIDTH]);
    }

    #[test]
    fn fit_columns_leaves_a_table_that_already_fits() {
        let mut w = vec![3, 5];
        fit_columns(&mut w, 40);
        assert_eq!(w, [3, 5]);
    }

    fn aligned(text: &str, width: usize, align: TableAlignment) -> String {
        align_cell(vec![Span::raw(text.to_string())], width, align)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn align_cell_pads_left_right_and_center() {
        assert_eq!(aligned("ab", 6, TableAlignment::Left), "ab    ");
        assert_eq!(aligned("ab", 6, TableAlignment::None), "ab    ");
        assert_eq!(aligned("ab", 6, TableAlignment::Right), "    ab");
        assert_eq!(aligned("ab", 6, TableAlignment::Center), "  ab  ");
    }

    /// Odd slack splits `slack / 2` left and the remainder right, so the cell
    /// still comes out exactly `width` columns wide.
    #[test]
    fn align_cell_centers_odd_slack_without_losing_a_column() {
        assert_eq!(aligned("ab", 7, TableAlignment::Center), "  ab   ");
    }

    #[test]
    fn align_cell_never_truncates() {
        assert_eq!(aligned("abcdefgh", 3, TableAlignment::Right), "abcdefgh");
    }
}
