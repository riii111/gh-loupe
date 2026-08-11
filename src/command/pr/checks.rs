use std::time::{Duration, Instant};

use crate::error::{Exit, Result};
use crate::model::CheckDiagnosticsOptions;
use crate::usecase;

pub(super) fn parse_args<I>(
    program: &str,
    values: std::iter::Peekable<I>,
) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut required = false;
    let mut failed_diagnostics = false;
    let mut include_failed_logs = false;
    let mut timeout_seconds = 90;
    let mut quiet = false;
    let parsed = super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, values| {
            if let Some(value) = super::exact_long_option_value(option, "--timeout") {
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
                        return Err(argument_error(
                            program,
                            "argument --timeout: expected one argument",
                        ));
                    };
                    if value.starts_with('-') {
                        return Err(argument_error(
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
        return Err(argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Checks {
            required,
            diagnostics: CheckDiagnosticsOptions {
                failed_diagnostics,
                include_failed_logs,
                timeout_seconds,
                quiet,
            },
        }),
        target,
        repo: parsed.repo,
        compact: parsed.compact,
        program: program.to_owned(),
    })
}

pub(super) fn execute(
    target_value: &str,
    repo: Option<String>,
    program: &str,
    required: bool,
    diagnostics: CheckDiagnosticsOptions,
) -> Result<serde_json::Value> {
    let target = super::super::target::resolve_pr_subcommand_target(
        target_value,
        repo,
        program,
        argument_error,
    )?;
    usecase::pull_request::checks::execute(&target, required, diagnostics)
}

fn parse_timeout(program: &str, value: &str) -> Result<u64> {
    let seconds = value
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| {
            argument_error(program, "argument --timeout: expected a positive integer")
        })?;
    if Instant::now()
        .checked_add(Duration::from_secs(seconds))
        .is_none()
    {
        return Err(argument_error(
            program,
            "argument --timeout: value cannot be represented as a diagnostic deadline",
        ));
    }
    Ok(seconds)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr checks [-h] [--repo REPO] [--required] [--failed-diagnostics] [--include-failed-logs] [--timeout SECONDS] [--quiet] [--compact] target"
    )
}

fn argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr checks: error: {message}",
            usage(program)
        )),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target                 PR number or GitHub pull request URL\n\noptions:\n  -h, --help             show this help message and exit\n  --repo REPO            OWNER/REPO; inferred from cwd when omitted\n  --required             only return required checks\n  --failed-diagnostics   include annotations for failed checks\n  --include-failed-logs  include annotations and bounded logs for failed checks\n  --timeout SECONDS      diagnostic timeout (default: 90)\n  --quiet                suppress diagnostic progress\n  --compact              emit one-line JSON\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}

#[cfg(test)]
mod tests {
    use super::super::super::{Action as RootAction, Args};
    use super::super::Action;
    use super::*;

    fn values(values: &[&str]) -> std::iter::Peekable<impl Iterator<Item = String>> {
        values.iter().map(|value| (*value).to_owned()).peekable()
    }

    #[test]
    fn parser_keeps_common_and_specific_options() {
        let args = parse_args(
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
                RootAction::Pr(Action::Checks {
                    required,
                    diagnostics,
                }),
            target,
            repo,
            compact,
            program,
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
        assert_eq!(program, "gh-read");
    }

    #[test]
    fn parser_rejects_unrepresentable_timeout() {
        let error = parse_timeout("gh-read", &u64::MAX.to_string())
            .expect_err("unrepresentable timeout must be rejected");

        assert_eq!(error.code, 2);
        assert!(error
            .stderr_line()
            .is_some_and(|line| line.contains("cannot be represented as a diagnostic deadline")));
    }
}
