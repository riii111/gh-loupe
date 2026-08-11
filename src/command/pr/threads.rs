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
    let mut include_resolved = false;
    let parsed = super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
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
        return Err(argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::Threads { include_resolved }),
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
    include_resolved: bool,
) -> Result<serde_json::Value> {
    let target = super::super::target::resolve_pr_subcommand_target(
        target_value,
        repo,
        program,
        argument_error,
    )?;
    Ok(output::success(serde_json::json!({
        "threads": usecase::pull_request::threads::execute(&target, include_resolved)?,
    })))
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr threads [-h] [--repo REPO] [--include-resolved] [--compact] target"
    )
}

fn argument_error(program: &str, message: &str) -> Exit {
    Exit {
        message: Some(format!(
            "{}\n{program} pr threads: error: {message}",
            usage(program)
        )),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-resolved  include resolved review threads\n  --compact           emit one-line JSON\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}
