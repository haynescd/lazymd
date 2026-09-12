use std::{fs, path::PathBuf};

use log::LevelFilter;
use simplelog::{Config, WriteLogger};

/// Sets up file-based logging for the TUI.
///
/// The TUI owns the terminal, so `println!` output is overwritten by the next
/// redraw before anyone sees it. Everything goes to `~/.codon/logs/codon.log`
/// instead, and `log::info!` and friends work crate-wide once this has run.
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
