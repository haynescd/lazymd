use std::process;

use clap::Parser;
use lazymd::{Args, run};

fn main() {
    if let Err(err) = run(Args::parse()) {
        eprintln!("lazymd: {err}");
        process::exit(1);
    }
}
