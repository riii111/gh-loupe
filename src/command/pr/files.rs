use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut limit = DEFAULT_LIMIT;
    let parsed = super::super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, values| {
            if let Some(value) = super::super::exact_long_option_value(option, "--limit") {
                limit = parse_limit(value, program)?;
                return Ok(true);
            }
            if option == "--limit" {
                let Some(value) = values.next() else {
                    return Err(argument_error(
                        program,
                        "argument --limit: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(argument_error(
                        program,
                        "argument --limit: expected one argument",
                    ));
                }
                limit = parse_limit(&value, program)?;
                Ok(true)
            } else {
                Ok(false)
            }
        },
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    super::super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Files { limit }),
        target,
        repo: parsed.repo,
        program: program.to_owned(),
    })
}

fn parse_limit(value: &str, program: &str) -> Result<usize> {
    let limit = value.parse::<usize>().ok();
    if !limit.is_some_and(|limit| (1..=MAX_LIMIT).contains(&limit)) {
        return Err(argument_error(
            program,
            "argument --limit: must be between 1 and 100",
        ));
    }
    Ok(limit.expect("limit was validated"))
}

pub(super) fn execute(target: &Target, limit: usize) -> Result<serde_json::Value> {
    usecase::pull_request::files::execute(target, limit)
}

fn usage(program: &str) -> String {
    format!("usage: {program} pr files [-h] [--repo REPO] [--limit N] target")
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(program, &usage(program), "pr files", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target       PR number or GitHub pull request URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --limit N    return at most N files, from 1 through 100 (default: 20)\n",
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
    fn parser_accepts_limit_and_common_options() {
        let args = parse_args(
            "gh-loupe",
            values(&["--repo=owner/repo", "--limit=7", "42"]),
        )
        .unwrap_or_else(|_| panic!("parse files arguments"));
        let Args {
            action: RootAction::Pr(Action::Files { limit }),
            target,
            repo,
            ..
        } = args
        else {
            panic!("unexpected action");
        };

        assert_eq!(limit, 7);
        assert_eq!(target, "42");
        assert_eq!(repo.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn parser_rejects_limits_outside_api_range() {
        for value in ["0", "101"] {
            let error = parse_limit(value, "gh-loupe").expect_err("invalid limit");
            assert_eq!(error.code, 2);
            assert!(error.stderr_line().contains("between 1 and 100"));
        }
    }
}
