use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;

use crate::error::{Exit, Result};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, Target};
use crate::output;
use crate::usecase;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Pr,
    Issue,
}

struct Args {
    action: Action,
    target: String,
    repo: Option<String>,
    compact: bool,
}

enum Action {
    PrInspect {
        include_resolved: bool,
    },
    PrChecks {
        required: bool,
        diagnostics: CheckDiagnosticsOptions,
    },
    PrOverview,
    PrThread {
        thread_id: String,
        include_diff_hunk: bool,
    },
    PrThreads {
        include_resolved: bool,
    },
    IssueInspect,
}

impl Action {
    const fn resource(&self) -> Resource {
        match self {
            Self::PrInspect { .. }
            | Self::PrChecks { .. }
            | Self::PrOverview
            | Self::PrThread { .. }
            | Self::PrThreads { .. } => Resource::Pr,
            Self::IssueInspect => Resource::Issue,
        }
    }
}

pub fn run() -> Result<()> {
    let Args {
        action,
        target,
        repo,
        compact,
    } = parse_args()?;
    let target = match &action {
        Action::PrChecks { .. } => {
            resolve_pr_subcommand_target(&target, repo, &program_name(), checks_argument_error)?
        }
        Action::PrOverview => {
            resolve_pr_subcommand_target(&target, repo, &program_name(), overview_argument_error)?
        }
        Action::PrThread { .. } => {
            resolve_pr_subcommand_target(&target, repo, &program_name(), thread_argument_error)?
        }
        Action::PrThreads { .. } => {
            resolve_pr_subcommand_target(&target, repo, &program_name(), threads_argument_error)?
        }
        _ => resolve_target(&target, repo, action.resource())?,
    };
    let result = match action {
        Action::PrChecks {
            required,
            diagnostics,
        } => usecase::pull_request::checks::execute(&target, required, diagnostics)?,
        Action::PrOverview => usecase::pull_request::overview::execute(&target)?,
        Action::PrThread {
            thread_id,
            include_diff_hunk,
        } => output::success(serde_json::json!({
            "thread": usecase::pull_request::thread::execute(
                &target,
                &thread_id,
                include_diff_hunk,
            )?,
        })),
        Action::PrThreads { include_resolved } => output::success(serde_json::json!({
            "threads": usecase::pull_request::threads::execute(&target, include_resolved)?,
        })),
        Action::PrInspect { include_resolved } => {
            usecase::pull_request::inspect::execute(&target, include_resolved, compact)?
        }
        Action::IssueInspect => usecase::issue::inspect::execute(&target)?,
    };
    let output = if compact {
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
    if resource == Resource::Pr && remaining.peek().is_some_and(|value| value == "checks") {
        remaining.next();
        return parse_checks_args(&program, remaining);
    }
    if resource == Resource::Pr && remaining.peek().is_some_and(|value| value == "overview") {
        remaining.next();
        return parse_overview_args(&program, remaining);
    }
    if resource == Resource::Pr && remaining.peek().is_some_and(|value| value == "threads") {
        remaining.next();
        return parse_threads_args(&program, remaining);
    }
    if resource == Resource::Pr && remaining.peek().is_some_and(|value| value == "thread") {
        remaining.next();
        return parse_thread_args(&program, remaining);
    }

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
                        Some(resource),
                        Some(resource),
                        "argument --repo: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(argument_error(
                        &program,
                        Some(resource),
                        Some(resource),
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
                print_help(&program, resource);
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
            Some(resource),
            Some(resource),
            "the following arguments are required: target",
        ));
    };
    if !unrecognized.is_empty() {
        return Err(argument_error(
            &program,
            None,
            None,
            &format!("unrecognized arguments: {}", unrecognized.join(" ")),
        ));
    }
    Ok(Args {
        action: match resource {
            Resource::Pr => Action::PrInspect { include_resolved },
            Resource::Issue => Action::IssueInspect,
        },
        target,
        repo,
        compact,
    })
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
    print_help: fn(&str),
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
                print_help(program);
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

fn parse_checks_args<I>(program: &str, values: std::iter::Peekable<I>) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut required = false;
    let mut failed_diagnostics = false;
    let mut include_failed_logs = false;
    let mut timeout_seconds = 90;
    let mut quiet = false;
    let parsed = parse_subcommand_args(
        program,
        values,
        1,
        checks_argument_error,
        print_checks_help,
        |option, values| {
            if let Some(value) = exact_long_option_value(option, "--timeout") {
                timeout_seconds = parse_timeout(program, value)?;
                return Ok(true);
            }
            match option {
                "--required" => required = true,
                "--failed-diagnostics" => failed_diagnostics = true,
                "--include-failed-logs" => {
                    include_failed_logs = true;
                    failed_diagnostics = true;
                }
                "--timeout" => {
                    let Some(value) = values.next() else {
                        return Err(checks_argument_error(
                            program,
                            "argument --timeout: expected one argument",
                        ));
                    };
                    if value.starts_with('-') {
                        return Err(checks_argument_error(
                            program,
                            "argument --timeout: expected one argument",
                        ));
                    }
                    timeout_seconds = parse_timeout(program, &value)?;
                }
                "--quiet" => quiet = true,
                _ => return Ok(false),
            }
            Ok(true)
        },
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(checks_argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    unrecognized_args(program, checks_argument_error, &parsed.unrecognized)?;
    Ok(Args {
        action: Action::PrChecks {
            required,
            diagnostics: CheckDiagnosticsOptions {
                failed_diagnostics,
                include_failed_logs,
                timeout_seconds,
                quiet,
            },
        },
        target,
        repo: parsed.repo,
        compact: parsed.compact,
    })
}

fn parse_overview_args<I>(program: &str, values: std::iter::Peekable<I>) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let parsed = parse_subcommand_args(
        program,
        values,
        1,
        overview_argument_error,
        print_overview_help,
        |_, _| Ok(false),
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(overview_argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    unrecognized_args(program, overview_argument_error, &parsed.unrecognized)?;
    Ok(Args {
        action: Action::PrOverview,
        target,
        repo: parsed.repo,
        compact: parsed.compact,
    })
}

fn parse_threads_args<I>(program: &str, values: std::iter::Peekable<I>) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut include_resolved = false;
    let parsed = parse_subcommand_args(
        program,
        values,
        1,
        threads_argument_error,
        print_threads_help,
        |option, _| match option {
            "--include-resolved" => {
                include_resolved = true;
                Ok(true)
            }
            _ => Ok(false),
        },
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(threads_argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    unrecognized_args(program, threads_argument_error, &parsed.unrecognized)?;
    Ok(Args {
        action: Action::PrThreads { include_resolved },
        target,
        repo: parsed.repo,
        compact: parsed.compact,
    })
}

fn parse_thread_args<I>(program: &str, values: std::iter::Peekable<I>) -> Result<Args>
where
    I: Iterator<Item = String>,
{
    let mut include_diff_hunk = false;
    let parsed = parse_subcommand_args(
        program,
        values,
        2,
        thread_argument_error,
        print_thread_help,
        |option, _| match option {
            "--include-diff-hunk" => {
                include_diff_hunk = true;
                Ok(true)
            }
            _ => Ok(false),
        },
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(thread_argument_error(
            program,
            "the following arguments are required: target, thread_id",
        ));
    };
    let Some(thread_id) = positionals.next() else {
        return Err(thread_argument_error(
            program,
            "the following arguments are required: thread_id",
        ));
    };
    unrecognized_args(program, thread_argument_error, &parsed.unrecognized)?;
    Ok(Args {
        action: Action::PrThread {
            thread_id,
            include_diff_hunk,
        },
        target,
        repo: parsed.repo,
        compact: parsed.compact,
    })
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

fn resolve_pr_subcommand_target(
    target: &str,
    repo: Option<String>,
    program: &str,
    argument_error: fn(&str, &str) -> Exit,
) -> Result<Target> {
    if let Some((url_repo, number)) = parse_url(target, Resource::Pr) {
        if !is_repo(url_repo) {
            return Err(argument_error(
                program,
                "pr URL must contain a valid OWNER/REPO",
            ));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            return Err(argument_error(
                program,
                "--repo conflicts with the pull request URL",
            ));
        }
        let Some(number) = positive_number(number) else {
            return Err(argument_error(
                program,
                "pr must be a positive number or GitHub pr URL",
            ));
        };
        return Ok(Target {
            repository: url_repo.to_owned(),
            number,
        });
    }

    let Some(number) = positive_number(target) else {
        return Err(argument_error(
            program,
            "pr must be a positive number or GitHub pr URL",
        ));
    };
    let repository = match repo {
        Some(repo) => repo,
        None => github::current_repository_runtime()?,
    };
    if !is_repo(&repository) {
        return Err(argument_error(program, "--repo must use OWNER/REPO format"));
    }
    Ok(Target { repository, number })
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

fn usage(program: &str, resource: Option<Resource>) -> String {
    match resource {
        Some(Resource::Pr) => format!(
            "usage: {program} pr [-h] [--repo REPO] [--include-resolved] [--compact] target"
        ),
        Some(Resource::Issue) => {
            format!("usage: {program} issue [-h] [--repo REPO] [--compact] target")
        }
        None => format!("usage: {program} [-h] {{pr,issue}} ..."),
    }
}

fn argument_error(
    program: &str,
    usage_resource: Option<Resource>,
    error_resource: Option<Resource>,
    message: &str,
) -> Exit {
    let usage = usage(program, usage_resource);
    let error_program = match error_resource {
        Some(Resource::Pr) => format!("{program} pr"),
        Some(Resource::Issue) => format!("{program} issue"),
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

fn parse_timeout(program: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| {
            checks_argument_error(program, "argument --timeout: expected a positive integer")
        })
}

fn checks_usage(program: &str) -> String {
    format!(
        "usage: {program} pr checks [-h] [--repo REPO] [--required] [--failed-diagnostics] [--include-failed-logs] [--timeout SECONDS] [--quiet] [--compact] target"
    )
}

fn checks_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr checks: error: {message}",
            checks_usage(program)
        )),
        code: 2,
    }
}

fn threads_usage(program: &str) -> String {
    format!(
        "usage: {program} pr threads [-h] [--repo REPO] [--include-resolved] [--compact] target"
    )
}

fn threads_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr threads: error: {message}",
            threads_usage(program)
        )),
        code: 2,
    }
}

fn thread_usage(program: &str) -> String {
    format!(
        "usage: {program} pr thread [-h] [--repo REPO] [--include-diff-hunk] [--compact] target thread_id"
    )
}

fn thread_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr thread: error: {message}",
            thread_usage(program)
        )),
        code: 2,
    }
}

fn overview_usage(program: &str) -> String {
    format!("usage: {program} pr overview [-h] [--repo REPO] [--compact] target")
}

fn overview_argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr overview: error: {message}",
            overview_usage(program)
        )),
        code: 2,
    }
}

fn program_name() -> String {
    let value = env::args().next().unwrap_or_else(|| "gh-read".to_owned());
    Path::new(&value)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("gh-read")
        .to_owned()
}

fn print_root_help(program: &str) {
    let text = format!(
        "{}\n\nRead fixed GitHub PR and Issue metadata without mutations.\n\npositional arguments:\n  {{pr,issue}}\n    pr        read pull request metadata and review data\n    issue     read issue metadata and comments\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program, None)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_help(program: &str, resource: Resource) {
    let text = match resource {
        Resource::Pr => format!(
            "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           omit repeated diff hunks and emit compact JSON\n",
            usage(program, Some(resource))
        ),
        Resource::Issue => format!(
            "{}\n\npositional arguments:\n  target       Issue number or GitHub issue URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
            usage(program, Some(resource))
        ),
    };
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_checks_help(program: &str) {
    let text = format!(
        "{}\n\npositional arguments:\n  target                 PR number or GitHub pull request URL\n\noptions:\n  -h, --help             show this help message and exit\n  --repo REPO            OWNER/REPO; inferred from cwd when omitted\n  --required             only return required checks\n  --failed-diagnostics   include annotations for failed checks\n  --include-failed-logs  include annotations and bounded logs for failed checks\n  --timeout SECONDS      diagnostic timeout (default: 90)\n  --quiet                suppress diagnostic progress\n  --compact              emit one-line JSON\n",
        checks_usage(program)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_threads_help(program: &str) {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           emit one-line JSON\n",
        threads_usage(program)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_thread_help(program: &str) {
    let text = format!(
        "{}\n\npositional arguments:\n  target               PR number or GitHub pull request URL\n  thread_id            GraphQL review thread node ID\n\noptions:\n  -h, --help           show this help message and exit\n  --repo REPO          OWNER/REPO; inferred from cwd when omitted\n  --include-diff-hunk  include diffHunk on every comment\n  --compact            emit one-line JSON\n",
        thread_usage(program)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn print_overview_help(program: &str) {
    let text = format!(
        "{}\n\npositional arguments:\n  target       PR number or GitHub pull request URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
        overview_usage(program)
    );
    io::stdout().write_all(text.as_bytes()).expect("write help");
}

fn stdout_line(message: &str) {
    writeln!(io::stdout(), "{message}").expect("write output");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(values: &[&str]) -> std::iter::Peekable<impl Iterator<Item = String>> {
        values.iter().map(|value| (*value).to_owned()).peekable()
    }

    #[test]
    fn subcommand_parser_keeps_common_and_specific_options() {
        let args = parse_checks_args(
            "gh-read",
            values(&[
                "--repo=owner/repo",
                "--required",
                "--include-failed-logs",
                "--timeout=12",
                "--quiet",
                "--compact",
                "42",
            ]),
        )
        .unwrap_or_else(|_| panic!("parse checks arguments"));
        let Args {
            action:
                Action::PrChecks {
                    required,
                    diagnostics,
                },
            target,
            repo,
            compact,
        } = args
        else {
            panic!("unexpected action");
        };

        assert!(required);
        assert!(diagnostics.failed_diagnostics);
        assert!(diagnostics.include_failed_logs);
        assert_eq!(diagnostics.timeout_seconds, 12);
        assert!(diagnostics.quiet);
        assert_eq!(target, "42");
        assert_eq!(repo.as_deref(), Some("owner/repo"));
        assert!(compact);
    }

    #[test]
    fn thread_parser_preserves_end_of_options_and_missing_value_boundaries() {
        let result = parse_thread_args("gh-read", values(&["--", "42", "thread-id", "--repo"]));
        let Err(error) = result else {
            panic!("expected an argument error")
        };

        assert_eq!(error.code, 2);
        assert!(
            error
                .stderr_line()
                .is_some_and(|message| message.contains("unrecognized arguments: --repo"))
        );

        let result = parse_thread_args("gh-read", values(&["42", "thread-id", "--repo"]));
        let Err(error) = result else {
            panic!("expected a missing-value error")
        };
        assert!(
            error
                .stderr_line()
                .is_some_and(|message| message.contains("argument --repo: expected one argument"))
        );
    }

    #[test]
    fn common_pr_target_resolver_keeps_subcommand_specific_errors() {
        for (argument_error, subcommand) in [
            (checks_argument_error as fn(&str, &str) -> Exit, "checks"),
            (overview_argument_error, "overview"),
        ] {
            let result = resolve_pr_subcommand_target(
                "0",
                Some("owner/repo".to_owned()),
                "gh-read",
                argument_error,
            );
            let Err(error) = result else {
                panic!("expected a target error")
            };
            assert_eq!(error.code, 2);
            assert!(error.stderr_line().is_some_and(|message| {
                message.contains(&format!("gh-read pr {subcommand}: error:"))
            }));
        }
    }
}
