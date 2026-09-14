use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use unicode_width::UnicodeWidthStr;

use crate::{app::App, theme};

/// Key hints shown in the status line when there's no message to show.
const HINTS: [(&str, &str); 5] = [
    ("q", "quit"),
    ("j/k", "scroll"),
    ("d/u", "half page"),
    ("g/G", "top/bottom"),
    ("r", "reload"),
];

pub fn ui(app: &mut App, frame: &mut Frame) {
    let [main, status] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    let title = Line::from(vec![
        Span::styled(" codon ", theme::title()),
        Span::styled("│ ", theme::border()),
        Span::styled(
            format!("{} ", app.file_name()),
            Style::new().add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(title);
    // One column of breathing room between the border and the text.
    let content = block.inner(main).inner(Margin::new(1, 0));

    // Layout first: it may re-render at a new width, and everything below
    // reads the result.
    app.set_viewport(content.width as usize, content.height as usize);

    frame.render_widget(block, main);
    frame.render_widget(Paragraph::new(app.visible_lines().to_vec()), content);

    if app.max_scroll() > 0 {
        // content_length counts scroll *positions*, so the thumb reaches the
        // bottom of the track exactly when the last line is on screen.
        let mut state = ScrollbarState::new(app.max_scroll() + 1)
            .position(app.scroll)
            .viewport_content_length(app.viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(theme::border())
            .thumb_symbol("┃")
            .thumb_style(theme::status_key());
        // Drawn over the block's right border, between the corners.
        frame.render_stateful_widget(scrollbar, main.inner(Margin::new(0, 1)), &mut state);
    }

    status_line(app, frame, status);
}

fn status_line(app: &App, frame: &mut Frame, area: Rect) {
    let right = Line::styled(format!("  {} ", position(app)), theme::status());
    let room = (area.width as usize).saturating_sub(right.width());

    let left = match &app.message {
        Some(m) => {
            let style = if m.is_error {
                theme::status_err()
            } else {
                theme::status_ok()
            };
            Line::styled(format!(" {}", m.text), style)
        }
        None => {
            // Show as many whole hints as fit, rather than cutting one off mid-word.
            let mut line = Line::from(" ");
            for (key, action) in HINTS {
                let key = Span::styled(key, theme::status_key());
                let action = Span::styled(format!(" {action}  "), theme::status());
                // The gap after the last hint may be clipped; the hint may not.
                let needed = key.width() + action.content.trim_end().width();
                if line.width() + needed > room {
                    break;
                }
                line.push_span(key);
                line.push_span(action);
            }
            line
        }
    };

    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right.width() as u16),
    ])
    .areas(area);
    frame.render_widget(left, left_area);
    frame.render_widget(right, right_area);
}

/// Where we are in the document, `less`-style: the visible line range, then
/// `All` / `Top` / `Bot` / a percentage.
fn position(app: &App) -> String {
    let total = app.lines.len();
    if total == 0 {
        return "empty".into();
    }
    let first = app.scroll + 1;
    // A viewport too short to show anything still reports a sane range.
    let last = (app.scroll + app.viewport_height.max(1)).min(total);
    let max = app.max_scroll();
    let where_ = if max == 0 {
        "All".to_string()
    } else if app.scroll == 0 {
        "Top".to_string()
    } else if app.scroll >= max {
        "Bot".to_string()
    } else {
        format!("{}%", app.scroll * 100 / max)
    };
    format!("{first}-{last}/{total}  {where_}")
}
