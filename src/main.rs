use std::{
    env::{self},
    process,
};

use codon::{Config, run};

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = Config::build(&args).unwrap_or_else(|err| {
        eprintln!("Problem parsing args: {err}");
        eprintln!("usage: codon <file.md>");
        process::exit(1);
    });

    if let Err(err) = run(config) {
        eprintln!("codon: {err}");
        process::exit(1);
    }
}
