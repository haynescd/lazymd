use std::{
    path::Path,
    time::{Duration, Instant},
};

use ratatui::text::Line;
use ratatui_image::{picker::Picker, sliced::SlicedProtocol};

use crate::render::{
    RenderElement,
    image::{Image, ImageLoader},
    render_ast,
};

/// How long a status-line message stays up.
const MESSAGE_TTL: Duration = Duration::from_secs(3);

/// A transient note shown in the status line ("reloaded", or an error).
#[derive(Debug)]
pub struct Message {
    pub text: String,
    pub is_error: bool,
    shown_at: Instant,
}

/// An image and the row of `App::lines` it starts on. The rows it covers are
/// blank in `lines`; the UI draws the image over them.
#[derive(Debug)]
pub struct PlacedImage {
    pub row: usize,
    pub image: Image,
}

pub struct App {
    pub should_quit: bool,
    pub md_filepath: String,
    /// The Markdown source, kept so it can be re-rendered when the width changes.
    source: String,
    /// Every row of the document, one per terminal row. An image's rows are
    /// blank placeholders, so scrolling treats them like any other line.
    pub lines: Vec<Line<'static>>,
    /// The images to draw over their placeholder rows, in document order.
    pub images: Vec<PlacedImage>,
    /// `None` shows images as their alt text (tests, or a terminal we couldn't query).
    image_loader: Option<ImageLoader>,
    /// Index of the first visible line.
    pub scroll: usize,
    pub viewport_height: usize,
    /// Width `lines` was rendered at, or `None` until the first frame is laid out.
    render_width: Option<usize>,
    pub message: Option<Message>,
}

impl App {
    pub fn new(md_filepath: String, source: String, picker: Option<Picker>) -> Self {
        let image_loader = picker.map(|p| ImageLoader::new(p, Path::new(&md_filepath)));
        App {
            should_quit: false,
            md_filepath,
            source,
            lines: Vec::new(),
            images: Vec::new(),
            image_loader,
            scroll: 0,
            viewport_height: 0,
            render_width: None,
            message: None,
        }
    }

    /// Expires the status message once it has been up long enough.
    pub fn tick(&mut self) {
        if self
            .message
            .as_ref()
            .is_some_and(|m| m.shown_at.elapsed() >= MESSAGE_TTL)
        {
            self.message = None;
        }
    }

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

    /// Tells the app how much room it has. Called every frame, but only
    /// re-renders when the width changed — that is, on a resize.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        if self.render_width != Some(width) {
            self.render_width = Some(width);
            self.rebuild();
        }
        self.viewport_height = height;
        self.clamp_scroll();
    }

    /// Re-renders the source at the current width.
    fn rebuild(&mut self) {
        let width = self.render_width.unwrap_or(80);
        let elements = render_ast(&self.source, width, self.image_loader.as_ref());

        self.lines.clear();
        self.images.clear();
        for element in elements {
            match element {
                RenderElement::Lines(lines) => self.lines.extend(lines),
                RenderElement::Image(image) => {
                    let row = self.lines.len();
                    let height = usize::from(image.height);
                    self.lines
                        .extend(std::iter::repeat_n(Line::default(), height));
                    self.images.push(PlacedImage { row, image });
                }
            }
        }
    }

    /// Swaps in new Markdown source, keeping the scroll position where possible.
    pub fn reload(&mut self, source: String) {
        self.source = source;
        if self.render_width.is_some() {
            self.rebuild();
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

    /// The images at least partly on screen, each with the viewport row it
    /// starts on — negative when its top has scrolled off.
    pub fn visible_images(&self) -> impl Iterator<Item = (&SlicedProtocol, i16)> {
        let (top, bottom) = (self.scroll, self.scroll + self.viewport_height);
        self.images
            .iter()
            .filter(move |p| p.row < bottom && p.row + usize::from(p.image.height) > top)
            // In range, so the offset is within ± one image or viewport height.
            .map(move |p| (&p.image.protocol, (p.row as i64 - top as i64) as i16))
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
        let mut app = App::new("dir/notes.md".into(), source, None);
        app.set_viewport(40, 10);
        app
    }

    /// An app whose document is one line, an image `rows` terminal rows tall
    /// (at halfblocks' 10x20 cells), and 30 more lines.
    fn app_with_image(rows: u32) -> App {
        // Tests run in parallel, so each call gets its own directory.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lazymd-test-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::new(10, rows * 20)
            .save(dir.join("pic.png"))
            .unwrap();

        let tail: String = (0..30).map(|i| format!("\n\nline {i}")).collect();
        let source = format!("top\n\n![pic](pic.png){tail}");
        let md_path = dir.join("doc.md").to_string_lossy().into_owned();
        let mut app = App::new(md_path, source, Some(Picker::halfblocks()));
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

    #[test]
    fn plain_text_has_no_images() {
        assert!(app().images.is_empty());
    }

    /// An image takes blank placeholder rows, so scrolling counts it like text.
    #[test]
    fn image_reserves_its_rows() {
        let app = app_with_image(4);
        assert_eq!(app.images.len(), 1);
        // "top", a blank separator, then the image.
        assert_eq!(app.images[0].row, 2);
        assert_eq!(app.images[0].image.height, 4);
        assert!(app.lines[2..6].iter().all(|l| l.width() == 0));
        // Then 30 lines, each after a blank separator.
        assert_eq!(app.lines.len(), 6 + 60);
    }

    #[test]
    fn visible_images_track_the_scroll() {
        let mut app = app_with_image(4);
        let offsets = |app: &App| app.visible_images().map(|(_, y)| y).collect::<Vec<_>>();
        assert_eq!(offsets(&app), [2]);
        app.scroll_down(3);
        // Top row scrolled off: the image starts above the viewport.
        assert_eq!(offsets(&app), [-1]);
        app.scroll_down(3);
        assert!(offsets(&app).is_empty());
    }
}
