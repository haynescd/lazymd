use std::{env, process};

use lazymd::{Command, USAGE, parse_args, run};

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = match parse_args(&args) {
        Ok(Command::Run(config)) => config,
        Ok(Command::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Command::Version) => {
            println!("lazymd {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(err) => {
            eprintln!("lazymd: {err}");
            eprintln!("Try 'lazymd --help' for more information.");
            process::exit(2);
        }
    };

    if let Err(err) = run(config) {
        eprintln!("lazymd: {err}");
        process::exit(1);
    }
}
