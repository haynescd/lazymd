use std::{env, fs, path::PathBuf, str::FromStr};

use anyhow::{Context, anyhow};
use log::LevelFilter;
use simplelog::{Config, WriteLogger};

/// Sets up file-based logging for the TUI.
///
/// The TUI owns the terminal, so `println!` output is overwritten by the next
/// redraw before anyone sees it. Everything goes to `codon.log` in the user's
/// state directory instead (see [`log_dir`]), truncated on each launch so it
/// only ever holds the latest session. `CODON_LOG` picks the level (`off`,
/// `error`, `warn`, `info`, `debug`, `trace`; default `info`), and `log::info!`
/// and friends work crate-wide once this has run.
pub fn init() -> anyhow::Result<()> {
    let level = match env::var("CODON_LOG") {
        Ok(value) => LevelFilter::from_str(&value)
            .map_err(|_| anyhow!("CODON_LOG={value} isn't a log level"))?,
        Err(_) => LevelFilter::Info,
    };
    if level == LevelFilter::Off {
        return Ok(());
    }

    let log_dir = log_dir().context("no state directory to log to")?;
    let log_path = log_dir.join("codon.log");
    let log_file = fs::create_dir_all(&log_dir)
        .and_then(|_| fs::File::create(&log_path))
        .with_context(|| format!("couldn't open {}", log_path.display()))?;

    WriteLogger::init(level, Config::default(), log_file)?;
    Ok(())
}

/// Where the log file lives: `$XDG_STATE_HOME/codon`, falling back to
/// `~/.local/state/codon` per the XDG spec, then `%LOCALAPPDATA%\codon` on
/// Windows.
fn log_dir() -> Option<PathBuf> {
    let from = |var: &str| {
        env::var_os(var)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    from("XDG_STATE_HOME")
        .or_else(|| from("HOME").map(|home| home.join(".local").join("state")))
        .or_else(|| from("LOCALAPPDATA"))
        .map(|dir| dir.join("codon"))
}
