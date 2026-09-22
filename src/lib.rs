use std::{error::Error, fs, path::PathBuf, str::FromStr};

use clap::{Parser, ValueEnum};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use log::LevelFilter;
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::{
    FontSize,
    picker::{Picker, ProtocolType},
};

use crate::{
    app::App,
    tui::Tui,
    tui::event::{Event, EventHandler},
    tui::watcher::MdWatcher,
};

pub mod app;
pub mod logging;
pub mod render;
pub mod theme;
pub mod tui;

/// Lines moved per mouse-wheel notch.
const WHEEL_STEP: usize = 3;

/// Cell size assumed when drawing kitty images without being able to ask the
/// terminal. ratatui-image's own guess; right-ish for a 12pt font at 1x scale.
/// A wrong guess draws images at the wrong size inside the space reserved for
/// them (blank margin, or cropped), which is why `--cell-size` exists.
const DEFAULT_CELL_SIZE: FontSize = FontSize::new(10, 20);

const KEYS_HELP: &str = "\
Keys:
  q, Esc, Ctrl-c   Quit
  j, k             Scroll down / up
  d, u             Half page down / up
  Space, b         Page down / up
  g, G             Jump to top / bottom
  r                Reload now
";

#[derive(Parser, Debug)]
#[command(
    version,
    about = "A terminal Markdown previewer with live reload.",
    after_help = KEYS_HELP
)]
pub struct Args {
    /// Markdown file to preview.
    pub md_file: PathBuf,

    /// How much is written to the log file.
    #[arg(
        long,
        env = "LAZYMD_LOG",
        default_value = "info",
        value_name = "LEVEL",
        value_parser = parse_log_level,
    )]
    pub log_level: LevelFilter,

    /// How images are drawn. `auto` asks the terminal; the others skip asking,
    /// for places where nothing can answer, like Neovim's :terminal.
    #[arg(
        long,
        env = "LAZYMD_IMAGE_PROTOCOL",
        default_value = "auto",
        value_name = "PROTOCOL"
    )]
    pub image_protocol: ImageProtocol,

    /// Terminal cell size in pixels, like 10x20. Only read with
    /// `--image-protocol kitty`, where lazymd can't ask the terminal for it.
    #[arg(
        long,
        env = "LAZYMD_CELL_SIZE",
        value_name = "WxH",
        value_parser = parse_cell_size,
    )]
    pub cell_size: Option<FontSize>,
}

/// How images are drawn. See [`picker`].
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Ask the terminal what it supports.
    Auto,
    /// Kitty graphics with Unicode placeholders, without asking first.
    Kitty,
    /// Two pixels per cell, drawn with half-block characters. Works anywhere.
    Halfblocks,
}

/// `LevelFilter`'s own parse error doesn't list the levels, so spell them out.
fn parse_log_level(value: &str) -> Result<LevelFilter, String> {
    LevelFilter::from_str(value)
        .map_err(|_| "expected one of: off, error, warn, info, debug, trace".to_string())
}

/// Parses `WxH`, like `10x20`. A zero either way is rejected: image layout
/// divides by it.
fn parse_cell_size(value: &str) -> Result<FontSize, String> {
    let err = || "expected WIDTHxHEIGHT in pixels, like 10x20".to_string();
    let (w, h) = value.split_once('x').ok_or_else(err)?;
    let w: u16 = w.parse().map_err(|_| err())?;
    let h: u16 = h.parse().map_err(|_| err())?;
    if w == 0 || h == 0 {
        return Err(err());
    }
    Ok(FontSize::new(w, h))
}

/// Decides how images are drawn.
///
/// `auto` asks the terminal, which needs a real one answering on stdin. Inside
/// Neovim's :terminal nothing answers, so the Neovim plugin passes
/// `--image-protocol kitty` and relays the image data to the outer terminal
/// itself. This reads replies from stdin, so it has to run before the event
/// thread starts.
fn picker(args: &Args) -> Picker {
    let picker = match args.image_protocol {
        ImageProtocol::Auto => Picker::from_query_stdio().unwrap_or_else(|e| {
            log::warn!("couldn't query terminal graphics, using halfblocks: {e}");
            Picker::halfblocks()
        }),
        ImageProtocol::Halfblocks => Picker::halfblocks(),
        ImageProtocol::Kitty => {
            // Not worth reading from the pty: Neovim leaves a :terminal's
            // pixel size at zero.
            let size = args.cell_size.unwrap_or(DEFAULT_CELL_SIZE);
            // Deprecated in favour of querying, which is exactly what can't
            // happen here. It's the only constructor that takes a known size.
            #[allow(deprecated)]
            let mut picker = Picker::from_fontsize(size);
            picker.set_protocol_type(ProtocolType::Kitty);
            picker
        }
    };

    // Logged every run: a standalone run at `--log-level info` is how you find
    // the real cell size to hand the Neovim plugin.
    let size = picker.font_size();
    log::info!(
        "images: {:?}, {}x{} px per cell",
        picker.protocol_type(),
        size.width,
        size.height
    );
    picker
}

pub fn run(args: Args) -> Result<(), Box<dyn Error>> {
    // Logs are a debugging aid; lazymd runs fine without them. This lands on the
    // main screen, so it's still there once the TUI exits.
    if let Err(e) = logging::init(args.log_level) {
        eprintln!("lazymd: logging disabled: {e}");
    }

    // App and the watcher both want a displayable path, not an OS string.
    let file_path = args.md_file.to_string_lossy().into_owned();
    log::info!("lazymd starting for {file_path}");

    // Read before touching the terminal, so a bad path is a plain error message.
    let contents =
        fs::read_to_string(&file_path).map_err(|e| format!("couldn't read {file_path}: {e}"))?;

    let picker = picker(&args);

    // Rendering waits for the first frame, when the terminal width is known.
    let mut app = App::new(file_path.clone(), contents, Some(picker));

    let watcher = MdWatcher::new(file_path.clone());
    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(250, watcher);
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;
    let result = (|| {
        while !app.should_quit {
            tui.draw(&mut app)?;
            match tui.events.next()? {
                Event::Key(ke) => handle_key_event(ke, &mut app),
                Event::Mouse(me) => handle_mouse_event(me, &mut app),
                // Nothing to do here: the next draw lays out at the new size,
                // re-rendering if the width changed.
                Event::Resize(w, h) => log::debug!("resized to {w}x{h}"),
                Event::Tick => app.tick(),
                Event::ReRender => reload(&mut app),
            }
        }
        Ok(())
    })();
    tui.exit()?;

    result
}

/// Re-reads the file. A failed read (say, mid-save) keeps the old render up
/// rather than taking the whole app down.
fn reload(app: &mut App) {
    log::info!("reloading {}", app.md_filepath);
    match fs::read_to_string(&app.md_filepath) {
        Ok(contents) => app.reload(contents),
        Err(e) => {
            log::warn!("reload failed: {e}");
            app.notify(format!("couldn't reload: {e}"), true);
        }
    }
}

fn handle_key_event(key_event: KeyEvent, app: &mut App) {
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
    match key_event.code {
        // Raw mode swallows SIGINT, so Ctrl-C has to be handled by hand.
        KeyCode::Char('c') if ctrl => app.quit(),
        KeyCode::Char('d') if ctrl => app.half_page_down(),
        KeyCode::Char('u') if ctrl => app.half_page_up(),
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
        KeyCode::Char('d') => app.half_page_down(),
        KeyCode::Char('u') => app.half_page_up(),
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => app.page_down(),
        KeyCode::PageUp | KeyCode::Char('b') => app.page_up(),
        KeyCode::Home | KeyCode::Char('g') => app.top(),
        KeyCode::End | KeyCode::Char('G') => app.bottom(),
        KeyCode::Char('r') => reload(app),
        _ => {}
    }
}

fn handle_mouse_event(mouse_event: MouseEvent, app: &mut App) {
    match mouse_event.kind {
        MouseEventKind::ScrollDown => app.scroll_down(WHEEL_STEP),
        MouseEventKind::ScrollUp => app.scroll_up(WHEEL_STEP),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("lazymd").chain(args.iter().copied()))
    }

    #[test]
    fn cli_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn one_file_runs() {
        // No assertion on `log_level`: it reads `LAZYMD_LOG` from the real
        // environment, so its default isn't ours to pin down here.
        assert_eq!(
            parse(&["notes.md"]).unwrap().md_file,
            PathBuf::from("notes.md")
        );
    }

    #[test]
    fn log_level_flag_wins_over_default_and_env() {
        assert_eq!(
            parse(&["--log-level", "debug", "notes.md"])
                .unwrap()
                .log_level,
            LevelFilter::Debug
        );
    }

    #[test]
    fn bad_log_level_is_an_error() {
        assert!(parse(&["--log-level", "loud", "notes.md"]).is_err());
    }

    #[test]
    fn unknown_option_is_an_error() {
        assert!(parse(&["--bogus", "notes.md"]).is_err());
    }

    #[test]
    fn no_file_is_an_error() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn two_files_is_an_error() {
        assert!(parse(&["a.md", "b.md"]).is_err());
    }

    /// `--` ends option parsing, so a file whose name starts with a dash opens.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(
            parse(&["--", "-notes.md"]).unwrap().md_file,
            PathBuf::from("-notes.md")
        );
    }

    // No default assertion for `image_protocol`, for the same reason as
    // `log_level`: `LAZYMD_IMAGE_PROTOCOL` comes from the real environment.
    #[test]
    fn image_protocol_flag_parses() {
        assert_eq!(
            parse(&["--image-protocol", "kitty", "notes.md"])
                .unwrap()
                .image_protocol,
            ImageProtocol::Kitty
        );
    }

    #[test]
    fn unknown_image_protocol_is_an_error() {
        assert!(parse(&["--image-protocol", "sixel", "notes.md"]).is_err());
    }

    #[test]
    fn cell_size_parses() {
        let size = parse_cell_size("12x26").unwrap();
        assert_eq!((size.width, size.height), (12, 26));
    }

    #[test]
    fn malformed_cell_sizes_are_errors() {
        for bad in [
            "0x20", "10x0", "abc", "10", "10x", "x20", "-1x20", "10x20x3",
        ] {
            assert!(parse_cell_size(bad).is_err(), "{bad} should be rejected");
        }
    }
}
