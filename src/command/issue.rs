use crate::error::{Exit, Result};
use crate::usecase;

pub(super) fn parse<I>(program: &str, mut remaining: I) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let mut target = None;
    let mut repo = None;
    let mut compact = false;
    let mut positional_only = false;
    let mut unrecognized = Vec::new();
    while let Some(value) = remaining.next() {
        if positional_only {
            if target.is_none() {
                target = Some(value);
            } else {
                unrecognized.push(value);
            }
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            option if super::exact_long_option_value(option, "--repo").is_some() => {
                repo = super::exact_long_option_value(option, "--repo").map(str::to_owned);
            }
            "--repo" => {
                let Some(value) = remaining.next() else {
                    return Err(issue_argument_error(
                        program,
                        "argument --repo: expected one argument",
                    ));
                };
                if value != "-" && value.starts_with('-') {
                    return Err(issue_argument_error(
                        program,
                        "argument --repo: expected one argument",
                    ));
                }
                repo = Some(value);
            }
            "--compact" => compact = true,
            "-h" | "--help" => {
                print_help(program)?;
                std::process::exit(0);
            }
            option if option.starts_with('-') => unrecognized.push(option.to_owned()),
            value if target.is_none() => target = Some(value.to_owned()),
            value => unrecognized.push(value.to_owned()),
        }
    }
    let Some(target) = target else {
        return Err(issue_argument_error(
            program,
            "the following arguments are required: target",
        ));
    };
    if !unrecognized.is_empty() {
        return Err(issue_argument_error(
            program,
            &format!("unrecognized arguments: {}", unrecognized.join(" ")),
        ));
    }
    Ok(super::Args {
        action: super::Action::Issue,
        target,
        repo,
        compact,
        program: program.to_owned(),
    })
}

pub(super) fn execute(target_value: &str, repo: Option<String>) -> Result<serde_json::Value> {
    let target = super::target::resolve_issue_target(target_value, repo)?;
    usecase::issue::inspect::execute(&target)
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
