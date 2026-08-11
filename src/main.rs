mod command;
mod error;
mod github;
mod model;
mod output;
mod usecase;

use std::io::{self, Write};

fn main() {
    if let Err(error) = command::run() {
        let _ = writeln!(io::stderr(), "{}", error.stderr_line());
        std::process::exit(error.code);
    }
}
