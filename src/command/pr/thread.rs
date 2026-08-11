use crate::error::{Exit, Result};
use crate::output;
use crate::usecase;

pub(super) fn parse_args<I>(
    program: &str,
    values: std::iter::Peekable<I>,
) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut include_diff_hunk = false;
    let parsed = super::parse_subcommand_args(
        program,
        values,
        2,
        argument_error,
        print_help,
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
        return Err(argument_error(
            program,
            "the following arguments are required: target, thread_id",
        ));
    };
    let Some(thread_id) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: thread_id",
        ));
    };
    super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Thread {
            thread_id,
            include_diff_hunk,
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
    thread_id: &str,
    include_diff_hunk: bool,
) -> Result<serde_json::Value> {
    let target = super::super::target::resolve_pr_subcommand_target(
        target_value,
        repo,
        program,
        argument_error,
    )?;
    Ok(output::success(serde_json::json!({
        "thread": usecase::pull_request::thread::execute(
            &target,
            thread_id,
            include_diff_hunk,
        )?,
    })))
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr thread [-h] [--repo REPO] [--include-diff-hunk] [--compact] target thread_id"
    )
}

fn argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr thread: error: {message}",
            usage(program)
        )),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target               PR number or GitHub pull request URL\n  thread_id            GraphQL review thread node ID\n\noptions:\n  -h, --help           show this help message and exit\n  --repo REPO          OWNER/REPO; inferred from cwd when omitted\n  --include-diff-hunk  include diffHunk on every comment\n  --compact            emit one-line JSON\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(values: &[&str]) -> std::iter::Peekable<impl Iterator<Item = String>> {
        values.iter().map(|value| (*value).to_owned()).peekable()
    }

    #[test]
    fn parser_preserves_end_of_options_and_missing_value_boundaries() {
        let result = parse_args("gh-read", values(&["--", "42", "thread-id", "--repo"]));
        let Err(error) = result else {
            panic!("expected an argument error")
        };

        assert_eq!(error.code, 2);
        assert!(
            error
                .stderr_line()
                .is_some_and(|message| message.contains("unrecognized arguments: --repo"))
        );

        let result = parse_args("gh-read", values(&["42", "thread-id", "--repo"]));
        let Err(error) = result else {
            panic!("expected a missing-value error")
        };
        assert!(
            error
                .stderr_line()
                .is_some_and(|message| message.contains("argument --repo: expected one argument"))
        );
    }
}
