use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut include_body = false;
    let mut include_details = false;
    let parsed = super::super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, _| match option {
            "--include-body" => {
                include_body = true;
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
            "the following arguments are required: target",
        ));
    };
    super::super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    if include_details && !include_body {
        return Err(argument_error(
            program,
            "argument --include-details: requires --include-body",
        ));
    }
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Overview {
            include_body,
            include_details,
        }),
        target,
        repo: parsed.repo,
        program: program.to_owned(),
    })
}

pub(super) fn execute(
    target: &Target,
    include_body: bool,
    include_details: bool,
) -> Result<serde_json::Value> {
    usecase::pull_request::overview::execute(target, include_body, include_details)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr overview [-h] [--repo REPO] [--include-body] [--include-details] target"
    )
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(program, &usage(program), "pr overview", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-body      include the pull request body\n  --include-details   include folded <details> content (requires --include-body)\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}
