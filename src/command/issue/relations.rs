use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;

const DEFAULT_LIMIT: i32 = 20;
const MAX_LIMIT: i32 = 100;

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
        action: super::super::Action::Issue(super::Action::Relations { limit }),
        target,
        repo: parsed.repo,
        compact: parsed.compact,
        program: program.to_owned(),
    })
}

fn print_help(program: &str) -> Result<()> {
    super::super::write_stdout(&format!(
        "{}\n  --limit N    limit each relation list to 1 through 100 items (default: 20)\n\nThe relation order is GitHub's connection order.\n",
        super::target_help(
            program,
            "relations",
            "Relations do not include Issue bodies or comments."
        )
    ))
}

fn parse_limit(value: &str, program: &str) -> Result<i32> {
    let limit = value.parse::<i32>().ok();
    if !limit.is_some_and(|limit| (1..=MAX_LIMIT).contains(&limit)) {
        return Err(argument_error(
            program,
            "argument --limit: must be between 1 and 100",
        ));
    }
    Ok(limit.expect("limit was validated"))
}

pub(super) fn execute(target: &Target, limit: i32) -> Result<serde_json::Value> {
    usecase::issue::relations::execute(target, limit)
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(
        program,
        &format!(
            "usage: {program} issue relations [-h] [--repo REPO] [--compact] [--limit N] target"
        ),
        "issue relations",
        message,
    )
}
