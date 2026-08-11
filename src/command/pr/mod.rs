mod checks;
mod comments;
mod overview;
mod review_thread;
mod review_threads;
mod reviews;

use crate::error::{Exit, Result};
use crate::model::CheckDiagnosticsOptions;

pub(super) enum Action {
    Checks {
        required: bool,
        diagnostics: CheckDiagnosticsOptions,
    },
    Comments,
    Overview,
    Reviews,
    ReviewThread {
        review_thread_id: String,
        include_diff_hunk: bool,
    },
    ReviewThreads {
        include_resolved: bool,
    },
}

pub(super) fn parse<I>(program: &str, mut remaining: std::iter::Peekable<I>) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let Some(subcommand) = remaining.next() else {
        return Err(pr_argument_error(
            program,
            "the following arguments are required: subcommand",
        ));
    };
    match subcommand.as_str() {
        "checks" => checks::parse_args(program, remaining),
        "comments" => comments::parse_args(program, remaining),
        "overview" => overview::parse_args(program, remaining),
        "reviews" => reviews::parse_args(program, remaining),
        "review-threads" => review_threads::parse_args(program, remaining),
        "review-thread" => review_thread::parse_args(program, remaining),
        "-h" | "--help" => {
            print_help(program)?;
            std::process::exit(0);
        }
        _ => Err(pr_argument_error(
            program,
            &format!(
                "argument subcommand: invalid choice: '{subcommand}' (choose from 'overview', 'comments', 'reviews', 'review-threads', 'review-thread', 'checks')"
            ),
        )),
    }
}

pub(super) fn execute(
    action: Action,
    target: &str,
    repo: Option<String>,
    program: &str,
) -> Result<serde_json::Value> {
    match action {
        Action::Checks {
            required,
            diagnostics,
        } => checks::execute(target, repo, program, required, diagnostics),
        Action::Comments => comments::execute(target, repo, program),
        Action::Overview => overview::execute(target, repo, program),
        Action::Reviews => reviews::execute(target, repo, program),
        Action::ReviewThread {
            review_thread_id,
            include_diff_hunk,
        } => review_thread::execute(target, repo, program, &review_thread_id, include_diff_hunk),
        Action::ReviewThreads { include_resolved } => {
            review_threads::execute(target, repo, program, include_resolved)
        }
    }
}

struct SubcommandArgs {
    positionals: Vec<String>,
    repo: Option<String>,
    compact: bool,
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
    let mut compact = false;
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
            "--compact" => compact = true,
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
        compact,
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

fn exact_long_option_value<'a>(value: &'a str, option: &str) -> Option<&'a str> {
    let (name, value) = value.split_once('=')?;
    (name == option).then_some(value)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr [-h] {{overview,comments,reviews,review-threads,review-thread,checks}} ..."
    )
}

fn pr_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: format!("{}\n{program} pr: error: {message}", usage(program)),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  {{overview,comments,reviews,review-threads,review-thread,checks}}\n    overview        read pull request state and summaries\n    comments        read pull request conversation comments\n    reviews         list pull request review submissions\n    review-threads  list review thread summaries\n    review-thread   read one review thread\n    checks          read individual checks and optional diagnostics\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program)
    );
    super::write_stdout(&text)
}
