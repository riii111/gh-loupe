mod issue;
mod pr;
mod target;

use std::env;
use std::io::{self, Write};

use crate::error::{Exit, Result};

const PROGRAM_NAME: &str = env!("CARGO_PKG_NAME");

struct Args {
    action: Action,
    target: String,
    repo: Option<String>,
    compact: bool,
    program: String,
}

enum Action {
    Pr(pr::Action),
    Issue,
}

pub fn run() -> Result<()> {
    let Args {
        action,
        target,
        repo,
        compact,
        program,
    } = parse_args()?;
    let result = match action {
        Action::Pr(action) => pr::execute(action, &target, repo, &program)?,
        Action::Issue => issue::execute(&target, repo)?,
    };
    let output = if compact {
        serde_json::to_string(&result)
    } else {
        serde_json::to_string_pretty(&result)
    }
    .map_err(|error| Exit::message(error.to_string()))?;
    write_stdout(&format!("{output}\n"))
}

fn parse_args() -> Result<Args> {
    let mut values = env::args();
    let program = displayed_program_name(values.next().as_deref()).to_owned();
    let Some(mut resource_value) = values.next() else {
        return Err(root_argument_error(
            &program,
            "the following arguments are required: resource",
        ));
    };
    if resource_value == "--version" {
        print_version()?;
        std::process::exit(0);
    }
    let root_positional_only = resource_value == "--";
    if root_positional_only {
        let Some(value) = values.next() else {
            return Err(root_argument_error(
                &program,
                "the following arguments are required: resource",
            ));
        };
        resource_value = value;
    }
    if !root_positional_only && (resource_value == "-h" || resource_value == "--help") {
        print_root_help(&program)?;
        std::process::exit(0);
    }

    let remaining = values.peekable();
    match resource_value.as_str() {
        "pr" => pr::parse(&program, remaining),
        "issue" => issue::parse(&program, remaining),
        other => Err(root_argument_error(
            &program,
            &format!("argument resource: invalid choice: '{other}' (choose from 'pr', 'issue')"),
        )),
    }
}

fn root_usage(program: &str) -> String {
    format!("usage: {program} [-h] [--version] {{pr,issue}} ...")
}

fn root_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program}: error: {message}",
            root_usage(program)
        )),
        code: 2,
    }
}

fn displayed_program_name(value: Option<&str>) -> &str {
    value
        .and_then(|value| value.rsplit('/').next())
        .filter(|value| !value.is_empty())
        .unwrap_or(PROGRAM_NAME)
}

fn print_root_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\nRead fixed GitHub PR and Issue metadata without mutations.\n\npositional arguments:\n  {{pr,issue}}\n    pr        read pull request metadata and review data\n    issue     read issue metadata and comments\n\noptions:\n  -h, --help  show this help message and exit\n  --version   show program's version and exit\n",
        root_usage(program)
    );
    write_stdout(&text)
}

fn print_version() -> Result<()> {
    let text = format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    write_stdout(&text)
}

fn write_stdout(text: &str) -> Result<()> {
    match io::stdout().write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(Exit::message(format!("failed to write stdout: {error}"))),
    }
}
