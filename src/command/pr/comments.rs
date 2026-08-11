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
    let parsed =
        super::parse_subcommand_args(program, values, 1, argument_error, print_help, |_, _| {
            Ok(false)
        })?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Comments),
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
) -> Result<serde_json::Value> {
    let target = super::super::target::resolve_pr_subcommand_target(
        target_value,
        repo,
        program,
        argument_error,
    )?;
    Ok(output::success(serde_json::json!({
        "comments": usecase::pull_request::comments::execute(&target)?,
    })))
}

fn usage(program: &str) -> String {
    format!("usage: {program} pr comments [-h] [--repo REPO] [--compact] target")
}

fn argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: format!(
            "{}\n{program} pr comments: error: {message}",
            usage(program)
        ),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target       PR number or GitHub pull request URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}
