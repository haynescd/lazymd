use std::{
    env::{self},
    process,
};

use codon::{Config, run};

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = Config::build(&args).unwrap_or_else(|err| {
        print!("Problem parsing args {err}");
        process::exit(1);
    });

    let _ = run(config);
}
