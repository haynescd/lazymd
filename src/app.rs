#[derive(Debug, Default)]
pub struct App {
    pub should_quit: bool,
    pub lines: Vec<String>,
    pub y_offset: u16,
}

impl App {
    pub fn new(lines: Vec<String>) -> Self {
        App {
            should_quit: false,
            lines,
            y_offset: 0,
        }
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {}

    /// Set should_quit to true to quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn up(&mut self) {
        self.y_offset = self.y_offset.saturating_sub(5);
    }

    pub fn down(&mut self) {
        self.y_offset += 5;
    }
}
