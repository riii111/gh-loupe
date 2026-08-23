use crate::error::{Exit, Result};
use crate::output;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Overview,
    Comments { include_details: bool },
    Relations { limit: i32 },
}

pub(super) fn parse<I>(program: &str, mut remaining: I) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let Some(subcommand) = remaining.next() else {
        return Err(issue_argument_error(
            program,
            "the following arguments are required: subcommand",
        ));
    };
    match subcommand.as_str() {
        "overview" => overview::parse_args(program, remaining),
        "comments" => comments::parse_args(program, remaining),
        "relations" => relations::parse_args(program, remaining),
        "-h" | "--help" => {
            print_help(program)?;
            std::process::exit(0);
        }
        target
            if super::target::is_target_like(
                target,
                super::target::Resource::Issue,
                super::target::positive_number,
            ) =>
        {
            Err(missing_subcommand_error(program, target))
        }
        _ => Err(issue_argument_error(
            program,
            &format!(
                "argument subcommand: invalid choice: '{subcommand}' (choose from 'overview', 'comments', 'relations')"
            ),
        )),
    }
}

pub(super) fn execute(
    action: Action,
    target_value: &str,
    repo: Option<String>,
    program: &str,
) -> Result<serde_json::Value> {
    let argument_error: fn(&str, &str) -> Exit = match action {
        Action::Overview => overview::argument_error,
        Action::Comments { .. } => comments::argument_error,
        Action::Relations { .. } => relations::argument_error,
    };
    let validate_number = match action {
        Action::Relations { .. } => positive_graphql_number,
        Action::Overview | Action::Comments { .. } => super::target::positive_number,
    };
    let target = super::target::resolve_target(
        target_value,
        repo,
        super::target::Resource::Issue,
        validate_number,
        argument_error,
        program,
    )?;
    let data = match action {
        Action::Overview => overview::execute(&target)?,
        Action::Comments { include_details } => comments::execute(&target, include_details)?,
        Action::Relations { limit } => relations::execute(&target, limit)?,
    };
    Ok(output::success(data))
}

fn usage(program: &str) -> String {
    format!("usage: {program} issue [-h] {{overview,comments,relations}} ...")
}

fn issue_argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(program, &usage(program), "issue", message)
}

fn missing_subcommand_error(program: &str, target: &str) -> Exit {
    Exit {
        message: format!(
            "{}\n{program} issue: error: a subcommand is required\n\nTry:\n  {program} issue overview {target}\n  {program} issue comments {target}\n  {program} issue relations {target}",
            usage(program)
        ),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  {{overview,comments,relations}}\n    overview   read Issue metadata and count summaries\n    comments   read Issue conversation comments\n    relations  read parent, sub-Issues, and dependencies\n\noptions:\n  -h, --help  show this help and exit\n",
        usage(program)
    );
    super::write_stdout(&text)
}

fn positive_graphql_number(value: &str) -> Option<String> {
    let value = super::target::positive_number(value)?;
    value.parse::<i32>().ok().map(|_| value)
}

fn parse_target_args<I>(
    program: &str,
    values: I,
    argument_error: fn(&str, &str) -> Exit,
    print_help: fn(&str) -> Result<()>,
) -> Result<(String, Option<String>, bool)>
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
    Ok((target, parsed.repo, parsed.compact))
}

fn target_usage(program: &str, command: &str) -> String {
    format!("usage: {program} issue {command} [-h] [--repo REPO] [--compact] target")
}

fn target_help(program: &str, command: &str, description: &str) -> String {
    format!(
        "{}\n\npositional arguments:\n  target       Issue number or GitHub issue URL\n\noptions:\n  -h, --help   show this help and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --compact    emit one-line JSON\n\n{}\n",
        target_usage(program, command),
        description
    )
}

mod comments;
mod overview;
mod relations;
