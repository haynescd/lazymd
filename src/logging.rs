use std::{fs, path::PathBuf};

use log::LevelFilter;
use simplelog::{Config, WriteLogger};

/// Sets up file-based logging for the TUI.
///
/// The TUI owns the whole terminal, so `println!`/`eprintln!` calls get
/// overwritten by the next redraw before you can ever see them. Everything
/// goes to a file instead: `~/.codon/logs/codon.log`. Once this is called,
/// `log::debug!`, `log::info!`, etc. work from anywhere in the crate.
pub fn init() -> anyhow::Result<()> {
    let log_dir = log_dir();
    fs::create_dir_all(&log_dir)?;

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("codon.log"))?;

    WriteLogger::init(LevelFilter::Debug, Config::default(), log_file)?;
    Ok(())
}

fn log_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".codon").join("logs")
}
