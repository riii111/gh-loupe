mod checks;
mod comments;
mod for_commit;
mod overview;
mod review_thread;
mod review_threads;
mod reviews;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::model::CheckDiagnosticsOptions;
use crate::output;

pub(super) enum Action {
    Checks {
        required: bool,
        diagnostics: CheckDiagnosticsOptions,
    },
    ForCommit {
        limit: usize,
    },
    Comments,
    Overview,
    Reviews {
        include_details: bool,
    },
    ReviewThread {
        review_thread_ids: Vec<String>,
        include_diff_hunk: bool,
        include_details: bool,
    },
    ReviewThreads {
        include_resolved: bool,
    },
}

pub(super) fn parse<I>(program: &str, mut remaining: I) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let Some(subcommand) = remaining.next() else {
        return Err(pr_argument_error(
            program,
            "the following arguments are required: subcommand",
        ));
    };
    match subcommand.as_str() {
        "checks" => checks::parse_args(program, remaining),
        "for-commit" => for_commit::parse_args(program, remaining),
        "comments" => comments::parse_args(program, remaining),
        "overview" => overview::parse_args(program, remaining),
        "reviews" => reviews::parse_args(program, remaining),
        "review-threads" => review_threads::parse_args(program, remaining),
        "review-thread" => review_thread::parse_args(program, remaining),
        "-h" | "--help" => {
            print_help(program)?;
            std::process::exit(0);
        }
        target
            if super::target::is_target_like(
                target,
                super::target::Resource::Pr,
                positive_pr_number,
            ) =>
        {
            Err(missing_subcommand_error(program, target))
        }
        _ => Err(pr_argument_error(
            program,
            &format!(
                "argument subcommand: invalid choice: '{subcommand}' (choose from 'overview', 'comments', 'reviews', 'review-threads', 'review-thread', 'checks', 'for-commit')"
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
    let argument_error: fn(&str, &str) -> Exit = match &action {
        Action::Checks { .. } => checks::argument_error,
        Action::ForCommit { .. } => for_commit::argument_error,
        Action::Comments => comments::argument_error,
        Action::Overview => overview::argument_error,
        Action::Reviews { .. } => reviews::argument_error,
        Action::ReviewThread { .. } => review_thread::argument_error,
        Action::ReviewThreads { .. } => review_threads::argument_error,
    };
    if let Action::ForCommit { limit } = &action {
        return for_commit::execute(target_value, repo, *limit, program);
    }
    let target = super::target::resolve_target(
        target_value,
        repo,
        super::target::Resource::Pr,
        positive_pr_number,
        argument_error,
        program,
    )?;
    let data = match action {
        Action::Checks {
            required,
            diagnostics,
        } => checks::execute(&target, required, diagnostics)?,
        Action::ForCommit { .. } => unreachable!("for-commit returns before resolving a PR target"),
        Action::Comments => Value::Object(Map::from_iter([(
            "comments".to_owned(),
            Value::Array(comments::execute(&target)?),
        )])),
        Action::Overview => overview::execute(&target)?,
        Action::Reviews { include_details } => Value::Object(Map::from_iter([(
            "reviews".to_owned(),
            Value::Array(reviews::execute(&target, include_details)?),
        )])),
        Action::ReviewThread {
            review_thread_ids,
            include_diff_hunk,
            include_details,
        } => Value::Object(Map::from_iter([(
            "reviewThreads".to_owned(),
            review_thread::execute(
                &target,
                &review_thread_ids,
                include_diff_hunk,
                include_details,
            )?,
        )])),
        Action::ReviewThreads { include_resolved } => Value::Object(Map::from_iter([(
            "reviewThreads".to_owned(),
            Value::Array(review_threads::execute(&target, include_resolved)?),
        )])),
    };
    Ok(output::success(data))
}

fn positive_pr_number(value: &str) -> Option<String> {
    let value = super::target::positive_number(value)?;
    value.parse::<i32>().ok().map(|_| value)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr [-h] {{overview,comments,reviews,review-threads,review-thread,checks,for-commit}} ..."
    )
}

fn pr_argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(program, &usage(program), "pr", message)
}

fn missing_subcommand_error(program: &str, target: &str) -> Exit {
    Exit {
        message: format!(
            "{}\n{program} pr: error: a subcommand is required\n\nTry:\n  {program} pr overview {target}\n  {program} pr comments {target}\n  {program} pr reviews {target}\n  {program} pr review-threads {target}\n  {program} pr checks {target}",
            usage(program)
        ),
        code: 2,
    }
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  {{overview,comments,reviews,review-threads,review-thread,checks,for-commit}}\n    overview        read pull request state and summaries\n    comments        read pull request conversation comments\n    reviews         list pull request review submissions\n    review-threads  list review thread summaries\n    review-thread   read review threads\n    checks          read individual checks and optional diagnostics\n    for-commit      find pull requests associated with a commit SHA\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program)
    );
    super::write_stdout(&text)
}
