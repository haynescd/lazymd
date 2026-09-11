use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::app::App;

pub fn ui(app: &mut App, frame: &mut Frame) {
    frame.render_widget(
        Paragraph::new(app.lines.join("\n"))
            .scroll((app.y_offset, 0))
            .block(
                Block::default()
                    .title("App")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        frame.area(),
    )
}
