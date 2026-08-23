mod issue;
mod pr;
mod search;
mod target;

use std::env;
use std::io::{self, Write};

use crate::error::{Exit, Result};

const PROGRAM_NAME: &str = env!("CARGO_PKG_NAME");

struct Args {
    action: Action,
    target: String,
    repo: Option<String>,
    program: String,
}

enum Action {
    Pr(pr::Action),
    Issue(issue::Action),
    Search(search::Action),
}

pub fn run() -> Result<()> {
    let Args {
        action,
        target,
        repo,
        program,
    } = parse_args()?;
    let result = match action {
        Action::Pr(action) => pr::execute(action, &target, repo, &program)?,
        Action::Issue(action) => issue::execute(action, &target, repo, &program)?,
        Action::Search(action) => search::execute(action, &target, repo, &program)?,
    };
    let output =
        serde_json::to_string(&result).map_err(|error| Exit::message(error.to_string()))?;
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

    match resource_value.as_str() {
        "pr" => pr::parse(&program, values),
        "issue" => issue::parse(&program, values),
        "search" => search::parse(&program, values),
        other => Err(root_argument_error(
            &program,
            &format!(
                "argument resource: invalid choice: '{other}' (choose from 'pr', 'issue', 'search')"
            ),
        )),
    }
}

fn root_usage(program: &str) -> String {
    format!("usage: {program} [-h] [--version] {{pr,issue,search}} ...")
}

fn root_argument_error(program: &str, message: &str) -> Exit {
    argument_error(program, &root_usage(program), "", message)
}

pub fn argument_error(program: &str, usage: &str, command: &str, message: &str) -> Exit {
    let command = if command.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {command}")
    };
    Exit {
        message: format!("{usage}\n{command}: error: {message}"),
        code: 2,
    }
}

pub fn exact_long_option_value<'a>(value: &'a str, option: &str) -> Option<&'a str> {
    let (name, value) = value.split_once('=')?;
    (name == option).then_some(value)
}

struct SubcommandArgs {
    positionals: Vec<String>,
    repo: Option<String>,
    unrecognized: Vec<String>,
}

fn parse_subcommand_args<I, F>(
    program: &str,
    mut values: I,
    positional_count: usize,
    argument_error: fn(&str, &str) -> Exit,
    print_help: fn(&str) -> Result<()>,
    mut parse_option: F,
) -> Result<SubcommandArgs>
where
    I: Iterator<Item = String>,
    F: FnMut(&str, &mut I) -> Result<bool>,
{
    let mut positionals = Vec::new();
    let mut repo = None;
    let mut positional_only = false;
    let mut unrecognized = Vec::new();

    while let Some(value) = values.next() {
        if positional_only {
            if positionals.len() < positional_count {
                positionals.push(value);
            } else {
                unrecognized.push(value);
            }
            continue;
        }
        if value == "--" {
            positional_only = true;
            continue;
        }
        if let Some(value) = exact_long_option_value(&value, "--repo") {
            repo = Some(value.to_owned());
            continue;
        }
        match value.as_str() {
            "--repo" => {
                let Some(value) = values.next() else {
                    return Err(argument_error(
                        program,
                        "argument --repo: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(argument_error(
                        program,
                        "argument --repo: expected one argument",
                    ));
                }
                repo = Some(value);
            }
            "-h" | "--help" => {
                print_help(program)?;
                std::process::exit(0);
            }
            option => {
                if parse_option(option, &mut values)? {
                    continue;
                }
                if option.starts_with('-') || positionals.len() >= positional_count {
                    unrecognized.push(value);
                } else {
                    positionals.push(value);
                }
            }
        }
    }

    Ok(SubcommandArgs {
        positionals,
        repo,
        unrecognized,
    })
}

fn unrecognized_args(
    program: &str,
    argument_error: fn(&str, &str) -> Exit,
    unrecognized: &[String],
) -> Result<()> {
    if unrecognized.is_empty() {
        Ok(())
    } else {
        Err(argument_error(
            program,
            &format!("unrecognized arguments: {}", unrecognized.join(" ")),
        ))
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
        "{}\n\nRead fixed GitHub PR and Issue metadata without mutations.\n\npositional arguments:\n  {{pr,issue,search}}\n    pr        read pull request metadata and review data\n    issue     read issue metadata, comments, and relations\n    search    search Issues or pull requests\n\noptions:\n  -h, --help  show this help message and exit\n  --version   show program's version and exit\n",
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
