use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event;
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};

use crate::watcher::MdWatcher;
/// Anything the main loop might wake up for.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// The watched file changed on disk.
    ReRender,
}

/// Polls the terminal and the file watcher on a background thread, and funnels
/// both into one channel.
#[derive(Debug)]
pub struct EventHandler {
    #[allow(dead_code)]
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
    #[allow(dead_code)]
    handler: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: u64, watcher: MdWatcher) -> Self {
        let tick_rate = Duration::from_millis(tick_rate);
        let (sender, receiver) = mpsc::channel();
        let handler = {
            let sender = sender.clone();
            thread::spawn(move || {
                let watcher = watcher; // force whole-struct capture, not just .watch_rx
                let mut last_tick = Instant::now();
                loop {
                    let timeout = tick_rate
                        .checked_sub(last_tick.elapsed())
                        .unwrap_or(tick_rate);
                    // A failed send means the receiver is gone and the app is
                    // shutting down, so this thread is done.
                    if event::poll(timeout).expect("unable to poll for event") {
                        let sent = match event::read().expect("unable to read event") {
                            CrosstermEvent::Key(e) if e.kind == event::KeyEventKind::Press => {
                                sender.send(Event::Key(e))
                            }
                            CrosstermEvent::Mouse(e) => sender.send(Event::Mouse(e)),
                            CrosstermEvent::Resize(w, h) => sender.send(Event::Resize(w, h)),
                            // Key releases/repeats, focus changes, pastes: not used.
                            _ => Ok(()),
                        };
                        if sent.is_err() {
                            return;
                        }
                    }
                    if last_tick.elapsed() >= tick_rate {
                        if sender.send(Event::Tick).is_err() {
                            return;
                        }

                        // Drain the pending fs-change signals, so the burst from one
                        // save (write + rename) collapses into a single re-render.
                        let mut changed = false;
                        while watcher.watch_rx.try_recv().is_ok() {
                            changed = true;
                        }
                        if changed && sender.send(Event::ReRender).is_err() {
                            return;
                        }
                        last_tick = Instant::now();
                    }
                }
            })
        };
        Self {
            sender,
            receiver,
            handler,
        }
    }
    pub fn next(&self) -> Result<Event> {
        Ok(self.receiver.recv()?)
    }
}
