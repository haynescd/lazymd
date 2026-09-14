use std::{env, process};

use codon::{Command, USAGE, parse_args, run};

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = match parse_args(&args) {
        Ok(Command::Run(config)) => config,
        Ok(Command::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Command::Version) => {
            println!("codon {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(err) => {
            eprintln!("codon: {err}");
            eprintln!("Try 'codon --help' for more information.");
            process::exit(2);
        }
    };

    if let Err(err) = run(config) {
        eprintln!("codon: {err}");
        process::exit(1);
    }
}
