use std::{error::Error, fs};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    app::App,
    event::{Event, EventHandler},
    tui::Tui,
    watcher::MdWatcher,
};

pub mod app;
pub mod event;
pub mod highlight;
pub mod logging;
pub mod render;
pub mod theme;
pub mod tui;
pub mod ui;
pub mod watcher;
pub mod wrap;

/// Lines moved per mouse-wheel notch.
const WHEEL_STEP: usize = 3;

#[derive(Debug)]
pub struct Config {
    pub file_path: String,
}

impl Config {
    pub fn build(args: &[String]) -> Result<Config, &'static str> {
        if args.len() < 2 {
            return Err("No md passed in");
        }

        let file_path = args[1].clone();
        Ok(Config { file_path })
    }
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    logging::init()?;
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
