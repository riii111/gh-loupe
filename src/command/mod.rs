use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;

use crate::error::{Exit, Result};
use crate::github;
use crate::model::Target;
use crate::output;
use crate::usecase;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Pr,
    Issue,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    PrInspect,
    PrThreads,
    IssueInspect,
}

struct Args {
    program: String,
    operation: Operation,
    resource: Resource,
    target: String,
    repo: Option<String>,
    include_resolved: bool,
    compact: bool,
}

pub fn run() -> Result<()> {
    let args = parse_args()?;
    let target = if args.operation == Operation::PrThreads {
        resolve_threads_target(&args.program, &args.target, args.repo)?
    } else {
        resolve_target(&args.target, args.repo, args.resource)?
    };
    let result = match args.operation {
        Operation::PrInspect => {
            usecase::pull_request::inspect::execute(&target, args.include_resolved, args.compact)?
        }
        Operation::PrThreads => output::success(serde_json::json!({
            "threads": usecase::pull_request::threads::execute(&target, args.include_resolved)?,
        })),
        Operation::IssueInspect => usecase::issue::inspect::execute(&target)?,
    };
    let output = if args.compact {
        serde_json::to_string(&result)
    } else {
        serde_json::to_string_pretty(&result)
    }
    .map_err(|error| Exit::message(error.to_string()))?;
    stdout_line(&output);
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut values = env::args();
    let program = values.next().unwrap_or_else(|| "gh-read".to_owned());
    let program = Path::new(&program)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gh-read")
        .to_owned();
    let Some(mut resource_value) = values.next() else {
        return Err(argument_error(
            &program,
            None,
            None,
            "the following arguments are required: resource",
        ));
    };
    let root_positional_only = resource_value == "--";
    if root_positional_only {
        let Some(value) = values.next() else {
            return Err(argument_error(
                &program,
                None,
                None,
                "the following arguments are required: resource",
            ));
        };
        resource_value = value;
    }
    if !root_positional_only && (resource_value == "-h" || resource_value == "--help") {
        print_root_help(&program);
        std::process::exit(0);
    }
    let resource = match resource_value.as_str() {
        "pr" => Resource::Pr,
        "issue" => Resource::Issue,
        other => {
            return Err(argument_error(
                &program,
                None,
                None,
                &format!(
                    "argument resource: invalid choice: '{other}' (choose from 'pr', 'issue')"
                ),
            ));
        }
    };

    let mut remaining = values.peekable();
    let operation = match resource {
        Resource::Pr if remaining.peek().is_some_and(|value| value == "threads") => {
            remaining.next();
            Operation::PrThreads
        }
        Resource::Pr => Operation::PrInspect,
        Resource::Issue => Operation::IssueInspect,
    };

    let mut target = None;
    let mut repo = None;
    let mut include_resolved = false;
    let mut compact = false;
    let mut positional_only = false;
    let mut unrecognized = Vec::new();
    while let Some(value) = remaining.next() {
        if positional_only {
            if target.is_none() {
                target = Some(value);
            } else {
                unrecognized.push(value);
            }
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            option if exact_long_option_value(option, "--repo").is_some() => {
                repo = exact_long_option_value(option, "--repo").map(str::to_owned);
            }
            "--repo" => {
                let Some(value) = remaining.next() else {
                    return Err(argument_error(
                        &program,
                        Some(operation),
                        Some(operation),
                        "argument --repo: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(argument_error(
                        &program,
                        Some(operation),
                        Some(operation),
                        "argument --repo: expected one argument",
                    ));
                }
                repo = Some(value);
            }
            "--include-resolved" if resource == Resource::Pr => {
                include_resolved = true;
            }
            "--compact" => compact = true,
            "-h" | "--help" => {
                print_help(&program, operation);
                std::process::exit(0);
            }
            option if option.starts_with('-') => unrecognized.push(option.to_owned()),
            value if target.is_none() => target = Some(value.to_owned()),
            value => unrecognized.push(value.to_owned()),
        }
    }
    let Some(target) = target else {
        return Err(argument_error(
            &program,
            Some(operation),
            Some(operation),
            "the following arguments are required: target",
        ));
    };
    if !unrecognized.is_empty() {
        let scoped_operation = (operation == Operation::PrThreads).then_some(operation);
        return Err(argument_error(
            &program,
            scoped_operation,
            scoped_operation,
            &format!("unrecognized arguments: {}", unrecognized.join(" ")),
        ));
    }
    Ok(Args {
        program,
        operation,
        resource,
        target,
        repo,
        include_resolved,
        compact,
    })
}

fn resolve_threads_target(program: &str, target: &str, repo: Option<String>) -> Result<Target> {
    if let Some((url_repo, number)) = parse_url(target, Resource::Pr) {
        if !is_repo(url_repo) {
            return Err(argument_error(
                program,
                Some(Operation::PrThreads),
                Some(Operation::PrThreads),
                "pr URL must contain a valid OWNER/REPO",
            ));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            return Err(argument_error(
                program,
                Some(Operation::PrThreads),
                Some(Operation::PrThreads),
                "--repo conflicts with the pull request URL",
            ));
        }
        if let Some(number) = positive_number(number) {
            return Ok(Target {
                repository: url_repo.to_owned(),
                number,
            });
        }
    }

    let Some(number) = positive_number(target) else {
        return Err(argument_error(
            program,
            Some(Operation::PrThreads),
            Some(Operation::PrThreads),
            "pr must be a positive number or GitHub pr URL",
        ));
    };
    let repository = match repo {
        Some(repo) if is_repo(&repo) => repo,
        Some(_) => {
            return Err(argument_error(
                program,
                Some(Operation::PrThreads),
                Some(Operation::PrThreads),
                "--repo must use OWNER/REPO format",
            ));
        }
        None => github::current_repository_runtime()?,
    };
    Ok(Target { repository, number })
}

fn resolve_target(target: &str, repo: Option<String>, resource: Resource) -> Result<Target> {
    if let Some((url_repo, number)) = parse_url(target, resource) {
        if !is_repo(url_repo) {
            let name = resource_name(resource);
            return Err(Exit::message(format!(
                "{name} URL must contain a valid OWNER/REPO"
            )));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            return Err(Exit::message("--repo conflicts with the pull request URL"));
        }
        if let Some(number) = positive_number(number) {
            return Ok(Target {
                repository: url_repo.to_owned(),
                number,
            });
        }
    }

    let Some(number) = positive_number(target) else {
        let name = resource_name(resource);
        return Err(Exit::message(format!(
            "{name} must be a positive number or GitHub {name} URL"
        )));
    };
    let repository = resolve_repo(repo)?;
    Ok(Target { repository, number })
}

fn resolve_repo(repo: Option<String>) -> Result<String> {
    let repo = match repo {
        Some(repo) => repo,
        None => github::current_repository()?,
    };
    if !is_repo(&repo) {
        return Err(Exit::message("--repo must use OWNER/REPO format"));
    }
    Ok(repo)
}

fn parse_url(target: &str, resource: Resource) -> Option<(&str, &str)> {
    let path = target.strip_prefix("https://github.com/")?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    let kind = segments.next()?;
    let number = segments.next()?;
    if segments.next().is_some()
        || kind
            != match resource {
                Resource::Pr => "pull",
                Resource::Issue => "issues",
            }
    {
        return None;
    }
    let repo_length = owner.len() + 1 + name.len();
    Some((&path[..repo_length], number))
}

fn positive_number(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = value.trim_start_matches('0');
    (!value.is_empty()).then(|| value.to_owned())
}

fn is_repo(value: &str) -> bool {
    let mut segments = value.split('/');
    let Some(owner) = segments.next() else {
        return false;
    };
    let Some(name) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && [owner, name].into_iter().all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        })
}

const fn resource_name(resource: Resource) -> &'static str {
    match resource {
        Resource::Pr => "pr",
        Resource::Issue => "issue",
    }
}

fn usage(program: &str, operation: Option<Operation>) -> String {
    match operation {
        Some(Operation::PrInspect) => format!(
            "usage: {program} pr [-h] [--repo REPO] [--include-resolved] [--compact] target"
        ),
        Some(Operation::PrThreads) => format!(
            "usage: {program} pr threads [-h] [--repo REPO] [--include-resolved] [--compact] target"
        ),
        Some(Operation::IssueInspect) => {
            format!("usage: {program} issue [-h] [--repo REPO] [--compact] target")
        }
        None => format!("usage: {program} [-h] {{pr,issue}} ..."),
    }
}

fn argument_error(
    program: &str,
    usage_operation: Option<Operation>,
    error_operation: Option<Operation>,
    message: &str,
) -> Exit {
    let usage = usage(program, usage_operation);
    let error_program = match error_operation {
        Some(Operation::PrInspect) => format!("{program} pr"),
        Some(Operation::PrThreads) => format!("{program} pr threads"),
        Some(Operation::IssueInspect) => format!("{program} issue"),
        None => program.to_owned(),
    };
    Exit {
        message: Some(format!("{usage}\n{error_program}: error: {message}")),
        code: 2,
    }
}

fn exact_long_option_value<'a>(value: &'a str, option: &str) -> Option<&'a str> {
    let (name, value) = value.split_once('=')?;
    (name == option).then_some(value)
}

fn print_root_help(program: &str) {
    let text = format!(
        "{}\n\nRead fixed GitHub PR and Issue metadata without mutations.\n\npositional arguments:\n  {{pr,issue}}\n    pr        read pull request metadata and review data\n    issue     read issue metadata and comments\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program, None)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_help(program: &str, operation: Operation) {
    let text = match operation {
        Operation::PrInspect => format!(
            "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           omit repeated diff hunks and emit compact JSON\n",
            usage(program, Some(operation))
        ),
        Operation::PrThreads => format!(
            "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           emit one-line JSON\n",
            usage(program, Some(operation))
        ),
        Operation::IssueInspect => format!(
            "{}\n\npositional arguments:\n  target       Issue number or GitHub issue URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
            usage(program, Some(operation))
        ),
    };
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn stdout_line(message: &str) {
    writeln!(io::stdout(), "{message}").expect("write output");
}
