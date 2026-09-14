use std::{error::Error, fs};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};

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

pub const USAGE: &str = "\
codon - a terminal Markdown previewer with live reload

Usage: codon [OPTIONS] <FILE>

Options:
  -h, --help       Print this help
  -V, --version    Print the version

Environment:
  CODON_LOG        Log level: off, error, warn, info (default), debug, trace

Keys:
  q, Esc, Ctrl-c   Quit
  j, k             Scroll down / up
  d, u             Half page down / up
  Space, b         Page down / up
  g, G             Jump to top / bottom
  r                Reload now
";

/// What the command line asked for.
#[derive(Debug, PartialEq)]
pub enum Command {
    Run(Config),
    Help,
    Version,
}

#[derive(Debug, PartialEq)]
pub struct Config {
    pub file_path: String,
}

/// Parses the command line, `args[0]` being the program name.
///
/// `--` ends option parsing, so `codon -- -notes.md` opens a file whose name
/// starts with a dash.
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut files = Vec::new();
    let mut options_done = false;
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            _ if options_done => files.push(arg),
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--" => options_done = true,
            opt if opt.starts_with('-') => return Err(format!("unknown option: {opt}")),
            _ => files.push(arg),
        }
    }

    match files.as_slice() {
        [file] => Ok(Command::Run(Config {
            file_path: file.to_string(),
        })),
        [] => Err("no file given".to_string()),
        _ => Err(format!("expected one file, got {}", files.len())),
    }
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    // Logs are a debugging aid; codon runs fine without them. This lands on the
    // main screen, so it's still there once the TUI exits.
    if let Err(e) = logging::init() {
        eprintln!("codon: logging disabled: {e}");
    }
    log::info!("codon starting for {}", config.file_path);

    let file_path = config.file_path.clone();
    // Read before touching the terminal, so a bad path is a plain error message.
    let contents =
        fs::read_to_string(&file_path).map_err(|e| format!("couldn't read {file_path}: {e}"))?;

    // Rendering waits for the first frame, when the terminal width is known.
    let mut app = App::new(file_path.clone(), contents);

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

    fn parse(args: &[&str]) -> Result<Command, String> {
        let args: Vec<String> = std::iter::once("codon")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        parse_args(&args)
    }

    fn run_cmd(file_path: &str) -> Result<Command, String> {
        Ok(Command::Run(Config {
            file_path: file_path.to_string(),
        }))
    }

    #[test]
    fn one_file_runs() {
        assert_eq!(parse(&["notes.md"]), run_cmd("notes.md"));
    }

    #[test]
    fn help_and_version_flags() {
        assert_eq!(parse(&["-h"]), Ok(Command::Help));
        assert_eq!(parse(&["--help"]), Ok(Command::Help));
        assert_eq!(parse(&["-V"]), Ok(Command::Version));
        assert_eq!(parse(&["--version"]), Ok(Command::Version));
    }

    #[test]
    fn help_after_file_still_wins() {
        assert_eq!(parse(&["notes.md", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn unknown_option_is_an_error() {
        assert_eq!(
            parse(&["--bogus", "notes.md"]),
            Err("unknown option: --bogus".to_string())
        );
    }

    #[test]
    fn no_file_is_an_error() {
        assert_eq!(parse(&[]), Err("no file given".to_string()));
    }

    #[test]
    fn two_files_is_an_error() {
        assert_eq!(
            parse(&["a.md", "b.md"]),
            Err("expected one file, got 2".to_string())
        );
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(parse(&["--", "-notes.md"]), run_cmd("-notes.md"));
        assert_eq!(parse(&["--", "--help"]), run_cmd("--help"));
    }
}
