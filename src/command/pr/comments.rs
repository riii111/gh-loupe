use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;
use serde_json::Value;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let mut include_details = false;
    let mut limit = None;
    let mut since = None;
    let parsed = super::super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, values| match option {
            "--include-details" => {
                include_details = true;
                Ok(true)
            }
            option => {
                if let Some(value) = super::super::exact_long_option_value(option, "--limit") {
                    limit = Some(parse_limit(value, program)?);
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
                    limit = Some(parse_limit(&value, program)?);
                    return Ok(true);
                }
                if let Some(value) = super::super::exact_long_option_value(option, "--since") {
                    since = Some(parse_since(value, program)?);
                    return Ok(true);
                }
                if option == "--since" {
                    let Some(value) = values.next() else {
                        return Err(argument_error(
                            program,
                            "argument --since: expected one argument",
                        ));
                    };
                    if value != "-" && value.starts_with('-') {
                        return Err(argument_error(
                            program,
                            "argument --since: expected one argument",
                        ));
                    }
                    since = Some(parse_since(&value, program)?);
                    return Ok(true);
                }
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
        action: super::super::Action::Pr(super::Action::Comments {
            include_details,
            limit,
            since,
        }),
        target,
        repo: parsed.repo,
        program: program.to_owned(),
    })
}

pub(super) fn execute(
    target: &Target,
    include_details: bool,
    limit: Option<usize>,
    since: Option<&str>,
) -> Result<serde_json::Value> {
    let result = usecase::pull_request::comments::execute(target, include_details, limit, since)?;
    let mut data = serde_json::Map::new();
    data.insert("comments".to_owned(), Value::Array(result.comments));
    if limit.is_some() || since.is_some() {
        data.insert("totalCount".to_owned(), Value::from(result.total_count));
        data.insert("truncated".to_owned(), Value::Bool(result.truncated));
    }
    Ok(Value::Object(data))
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr comments [-h] [--repo REPO] [--include-details] [--limit N] [--since TIMESTAMP] target"
    )
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(program, &usage(program), "pr comments", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  target              PR number or GitHub pull request URL\n\noptions:\n  -h, --help          show this help message and exit\n  --repo REPO         OWNER/REPO; inferred from cwd when omitted\n  --include-details   include folded <details> content (omitted by default)\n  --limit N           return the latest 1 through 100 comments\n  --since TIMESTAMP   return comments updated after TIMESTAMP\n",
        usage(program)
    );
    super::super::write_stdout(&text)
}

fn parse_limit(value: &str, program: &str) -> Result<usize> {
    let limit = value.parse::<usize>().ok();
    if !limit.is_some_and(|limit| (1..=100).contains(&limit)) {
        return Err(argument_error(
            program,
            "argument --limit: must be between 1 and 100",
        ));
    }
    Ok(limit.expect("limit was validated"))
}

fn parse_since(value: &str, program: &str) -> Result<String> {
    if value.is_empty() {
        return Err(argument_error(
            program,
            "argument --since: must not be empty",
        ));
    }
    Ok(value.to_owned())
}
