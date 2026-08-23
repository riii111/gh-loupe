use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;
use serde_json::Value;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut include_details = false;
    let parsed = super::super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, _| match option {
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
            "the following arguments are required: target",
        ));
    };
    super::super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Comments { include_details }),
        target,
        repo: parsed.repo,
        compact: parsed.compact,
        program: program.to_owned(),
    })
}

pub(super) fn execute(target: &Target, include_details: bool) -> Result<Vec<Value>> {
    usecase::pull_request::comments::execute(target, include_details)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr comments [-h] [--repo REPO] [--include-details] [--compact] target"
    )
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(program, &usage(program), "pr comments", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-details   include folded <details> content (omitted by default)\n  --compact           emit one-line JSON\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}
