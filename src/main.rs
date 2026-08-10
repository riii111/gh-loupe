mod command;
mod error;
mod github;
mod model;
mod usecase;

use std::io::{self, Write};

fn main() {
    if let Err(error) = command::run() {
        if let Some(message) = error.message {
            writeln!(io::stderr(), "{message}").expect("write error");
        }
        std::process::exit(error.code);
    }
}
