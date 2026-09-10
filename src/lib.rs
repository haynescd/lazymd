use std::fs;

use crate::render::render_ast;

pub mod render;

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

pub fn run(config: Config) -> std::io::Result<String> {
    let contents = fs::read_to_string(config.file_path)?;

    render_ast(&contents);

    Ok(contents)
}
