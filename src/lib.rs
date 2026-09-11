use std::{error::Error, fs};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    app::App,
    event::{Event, EventHandler},
    render::render_ast,
    tui::Tui,
    ui::ui,
    watcher::MdWatcher,
};

pub mod app;
pub mod event;
pub mod logging;
pub mod render;
pub mod tui;
pub mod ui;
pub mod watcher;

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
    let contents = fs::read_to_string(file_path.clone())?;
    let lines = render_ast(&contents);

    let mut app = App::new(file_path.clone(), lines);

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
                Event::Mouse(_) => {}
                Event::Resize(_, _) => {}
                Event::Tick => {}
                Event::ReRender => {
                    log::info!("reloading {}", app.md_filepath);
                    let contents = fs::read_to_string(&app.md_filepath)?;
                    let lines = render_ast(&contents);
                    app.update(lines);
                }
            }
        }
        Ok(())
    })();
    tui.exit()?;

    result
}

fn handle_key_event(key_event: KeyEvent, app: &mut App) {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('q') => app.quit(),
        KeyCode::Down | KeyCode::Char('j') => app.down(),
        KeyCode::Up | KeyCode::Char('k') => app.up(),
        _ => {}
    }
}
