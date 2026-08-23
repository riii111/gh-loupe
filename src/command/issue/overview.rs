use crate::error::{Exit, Result};
use crate::model::Target;
use crate::usecase;

pub(super) fn parse_args<I>(program: &str, values: I) -> Result<super::super::Args>
where
    I: Iterator<Item = String>,
{
    let (target, repo) = super::parse_target_args(program, values, argument_error, |program| {
        super::super::write_stdout(&super::target_help(
            program,
            "overview",
            "Issue overview excludes conversation comments.\n",
        ))
    })?;
    Ok(super::super::Args {
        action: super::super::Action::Issue(super::Action::Overview),
        target,
        repo,
        program: program.to_owned(),
    })
}

pub(super) fn execute(target: &Target) -> Result<serde_json::Value> {
    usecase::issue::inspect::overview(target)
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(
        program,
        &super::target_usage(program, "overview"),
        "issue overview",
        message,
    )
}
