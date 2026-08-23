use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut include_diff_hunk = false;
    let mut include_details = false;
    let parsed = super::super::parse_subcommand_args(
        program,
        values,
        usize::MAX,
        argument_error,
        print_help,
        |option, _| match option {
            "--include-diff-hunk" => {
                include_diff_hunk = true;
                Ok(true)
            }
            "--include-details" => {
                include_details = true;
                Ok(true)
            }
            _ => Ok(false),
        },
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: target, review_thread_id",
        ));
    };
    let review_thread_ids = positionals.collect::<Vec<_>>();
    if review_thread_ids.is_empty() {
        return Err(argument_error(
            program,
            "the following arguments are required: review_thread_id",
        ));
    }
    if review_thread_ids.len() > 20 {
        return Err(argument_error(
            program,
            "review_thread_id accepts at most 20 IDs",
        ));
    }
    for (index, review_thread_id) in review_thread_ids.iter().enumerate() {
        if review_thread_id.is_empty() {
            return Err(argument_error(
                program,
                "review_thread_id must not be empty",
            ));
        }
        if review_thread_ids[..index]
            .iter()
            .any(|previous| previous == review_thread_id)
        {
            return Err(argument_error(
                program,
                &format!("duplicate review_thread_id: {review_thread_id}"),
            ));
        }
    }
    super::super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::ReviewThread {
            review_thread_ids,
            include_diff_hunk,
            include_details,
        }),
        target,
        repo: parsed.repo,
        program: program.to_owned(),
    })
}

pub(super) fn execute(
    target: &Target,
    review_thread_ids: &[String],
    include_diff_hunk: bool,
    include_details: bool,
) -> Result<serde_json::Value> {
    usecase::pull_request::review_thread::execute(
        target,
        review_thread_ids,
        include_diff_hunk,
        include_details,
    )
    .map(serde_json::Value::Array)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr review-thread [-h] [--repo REPO] [--include-diff-hunk] [--include-details] target review_thread_id [review_thread_id ...]"
    )
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(program, &usage(program), "pr review-thread", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n  review_thread_id    one to 20 GraphQL review thread node IDs\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-diff-hunk include diffHunk on every comment\n  --include-details   include folded <details> content (omitted by default)\n",
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
    fn parser_preserves_unrecognized_and_missing_value_boundaries() {
        let result = parse_args(
            "gh-loupe",
            values(&["42", "review-thread-id", "--rep", "riii111/dotfiles"]),
        );
        let Err(error) = result else {
            panic!("expected an argument error")
        };

        assert_eq!(error.code, 2);
        assert!(
            error
                .stderr_line()
                .contains("unrecognized arguments: --rep")
        );

        let result = parse_args("gh-loupe", values(&["42", "review-thread-id", "--repo"]));
        let Err(error) = result else {
            panic!("expected a missing-value error")
        };
        assert!(
            error
                .stderr_line()
                .contains("argument --repo: expected one argument")
        );
    }
}
