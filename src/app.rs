use std::{
    path::Path,
    time::{Duration, Instant},
};

use ratatui::text::Line;

use crate::render::render_ast;

/// How long a status-line message stays up.
const MESSAGE_TTL: Duration = Duration::from_secs(3);

/// A transient note shown in the status line ("reloaded", or an error).
#[derive(Debug)]
pub struct Message {
    pub text: String,
    pub is_error: bool,
    shown_at: Instant,
}

#[derive(Debug)]
pub struct App {
    pub should_quit: bool,
    pub md_filepath: String,
    /// The Markdown source, kept so it can be re-rendered when the width changes.
    source: String,
    /// `source` rendered at `render_width` columns.
    pub lines: Vec<Line<'static>>,
    /// Index of the first visible line.
    pub scroll: usize,
    pub viewport_height: usize,
    /// Width `lines` was rendered at; 0 until the first frame is laid out.
    render_width: usize,
    pub message: Option<Message>,
}

impl App {
    pub fn new(md_filepath: String, source: String) -> Self {
        App {
            should_quit: false,
            md_filepath,
            source,
            lines: Vec::new(),
            scroll: 0,
            viewport_height: 0,
            render_width: 0,
            message: None,
        }
    }

    /// Handles the tick event of the terminal: expires the status message.
    pub fn tick(&mut self) {
        if self
            .message
            .as_ref()
            .is_some_and(|m| m.shown_at.elapsed() >= MESSAGE_TTL)
        {
            self.message = None;
        }
    }

    /// Set should_quit to true to quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Just the file's name, for the title bar.
    pub fn file_name(&self) -> &str {
        Path::new(&self.md_filepath)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.md_filepath)
    }

    /// Tells the app how much room it has. Called every frame by the UI; only
    /// re-renders when the width actually changed (i.e. on a resize).
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        if width != self.render_width {
            self.render_width = width;
            self.lines = render_ast(&self.source, width);
        }
        self.viewport_height = height;
        self.clamp_scroll();
    }

    /// Swaps in new Markdown source, keeping the scroll position where possible.
    pub fn reload(&mut self, source: String) {
        self.source = source;
        if self.render_width > 0 {
            self.lines = render_ast(&self.source, self.render_width);
        }
        self.clamp_scroll();
        self.notify("reloaded", false);
    }

    pub fn notify(&mut self, text: impl Into<String>, is_error: bool) {
        self.message = Some(Message {
            text: text.into(),
            is_error,
            shown_at: Instant::now(),
        });
    }

    // --- scrolling ---------------------------------------------------------

    /// The furthest we can scroll: the last line sits at the bottom of the view.
    pub fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.viewport_height)
    }

    pub fn visible_lines(&self) -> &[Line<'static>] {
        let end = (self.scroll + self.viewport_height).min(self.lines.len());
        &self.lines[self.scroll.min(end)..end]
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_add(n);
        self.clamp_scroll();
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.viewport_height.saturating_sub(1).max(1));
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.viewport_height.saturating_sub(1).max(1));
    }

    pub fn half_page_down(&mut self) {
        self.scroll_down((self.viewport_height / 2).max(1));
    }

    pub fn half_page_up(&mut self) {
        self.scroll_up((self.viewport_height / 2).max(1));
    }

    pub fn top(&mut self) {
        self.scroll = 0;
    }

    pub fn bottom(&mut self) {
        self.scroll = self.max_scroll();
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with 100 one-line paragraphs, shown 10 lines at a time.
    fn app() -> App {
        let source = (0..100)
            .map(|i| format!("line {i}\n\n"))
            .collect::<String>();
        let mut app = App::new("dir/notes.md".into(), source);
        app.set_viewport(40, 10);
        app
    }

    #[test]
    fn renders_on_first_layout() {
        let app = app();
        // 100 paragraphs separated by blank lines
        assert_eq!(app.lines.len(), 199);
        assert_eq!(app.visible_lines().len(), 10);
    }

    #[test]
    fn scrolling_stops_at_the_last_screenful() {
        let mut app = app();
        app.scroll_down(1000);
        assert_eq!(app.scroll, 189);
        assert_eq!(app.visible_lines().len(), 10);
        app.scroll_up(1000);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn paging_keeps_one_line_of_context() {
        let mut app = app();
        app.page_down();
        assert_eq!(app.scroll, 9);
        app.half_page_down();
        assert_eq!(app.scroll, 14);
        app.bottom();
        assert_eq!(app.scroll, app.max_scroll());
        app.top();
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn reload_that_shrinks_the_document_clamps_scroll() {
        let mut app = app();
        app.bottom();
        app.reload("short".into());
        assert_eq!(app.scroll, 0);
        assert_eq!(app.lines.len(), 1);
    }

    #[test]
    fn growing_the_viewport_clamps_scroll() {
        let mut app = app();
        app.bottom();
        app.set_viewport(40, 500);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn file_name_strips_directories() {
        assert_eq!(app().file_name(), "notes.md");
    }
}
