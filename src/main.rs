mod command;
mod error;
mod github;
mod model;
mod output;
mod usecase;

use std::io::{self, Write};

fn main() {
    if let Err(error) = command::run() {
        if let Some(message) = error.stderr_line() {
            let _ = writeln!(io::stderr(), "{message}");
        }
        std::process::exit(error.code);
    }
}
