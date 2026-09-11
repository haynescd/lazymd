use std::{error::Error, fs, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{app::App, render::render_ast, tui::Tui, ui::ui};

pub mod app;
pub mod render;
pub mod tui;
pub mod ui;

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
    let contents = fs::read_to_string(config.file_path)?;
    let lines = render_ast(&contents);
    let mut app = App::new(lines);

    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal);
    tui.enter()?;
    let result = (|| {
        while !app.should_quit {
            tui.draw(&mut app)?;
            handle_events(&mut app)?;
        }
        Ok(())
    })();
    tui.exit()?;

    result
}

fn handle_events(app: &mut App) -> io::Result<()> {
    match event::read()? {
        // it's important to check that the event is a key press event as
        // crossterm also emits key release and repeat events on Windows.
        Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            handle_key_event(key_event, app);
        }
        _ => {}
    };
    Ok(())
}

fn handle_key_event(key_event: KeyEvent, app: &mut App) {
    match key_event.code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('j') => app.down(),
        KeyCode::Char('k') => app.up(),
        _ => {}
    }
}
