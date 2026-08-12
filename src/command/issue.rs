use crate::error::{Exit, Result};
use crate::output;
use crate::usecase;

pub(super) fn parse<I>(program: &str, remaining: I) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let parsed = super::parse_subcommand_args(
        program,
        remaining,
        1,
        issue_argument_error,
        print_help,
        |_, _| Ok(false),
    )?;
    let mut positionals = parsed.positionals.into_iter();
    let Some(target) = positionals.next() else {
        return Err(issue_argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    super::unrecognized_args(program, issue_argument_error, &parsed.unrecognized)?;
    Ok(super::Args {
        action: super::Action::Issue,
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
    let target = super::target::resolve_target(
        target_value,
        repo,
        super::target::Resource::Issue,
        super::target::positive_number,
        issue_argument_error,
        program,
    )?;
    Ok(output::success(usecase::issue::inspect::execute(&target)?))
}

fn usage(program: &str) -> String {
    format!("usage: {program} issue [-h] [--repo REPO] [--compact] target")
}

fn issue_argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(program, &usage(program), "issue", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target       Issue number or GitHub issue URL\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n",
        usage(program)
    );
    super::write_stdout(&text)
}
