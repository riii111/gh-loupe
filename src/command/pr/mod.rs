mod checks;
mod comments;
mod overview;
mod review_thread;
mod review_threads;
mod reviews;

use crate::error::{Exit, Result};
use crate::model::CheckDiagnosticsOptions;
use crate::output;

pub(super) enum Action {
    Checks {
        required: bool,
        diagnostics: CheckDiagnosticsOptions,
    },
    Comments,
    Overview,
    Reviews,
    ReviewThread {
        review_thread_id: String,
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
        "comments" => comments::parse_args(program, remaining),
        "overview" => overview::parse_args(program, remaining),
        "reviews" => reviews::parse_args(program, remaining),
        "review-threads" => review_threads::parse_args(program, remaining),
        "review-thread" => review_thread::parse_args(program, remaining),
        "-h" | "--help" => {
            print_help(program)?;
            std::process::exit(0);
        }
        _ => Err(pr_argument_error(
            program,
            &format!(
                "argument subcommand: invalid choice: '{subcommand}' (choose from 'overview', 'comments', 'reviews', 'review-threads', 'review-thread', 'checks')"
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
        Action::Comments => comments::argument_error,
        Action::Overview => overview::argument_error,
        Action::Reviews => reviews::argument_error,
        Action::ReviewThread { .. } => review_thread::argument_error,
        Action::ReviewThreads { .. } => review_threads::argument_error,
    };
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
        Action::Comments => serde_json::json!({
            "comments": comments::execute(&target)?,
        }),
        Action::Overview => overview::execute(&target)?,
        Action::Reviews => serde_json::json!({
            "reviews": reviews::execute(&target)?,
        }),
        Action::ReviewThread {
            review_thread_id,
            include_diff_hunk,
            include_details,
        } => serde_json::json!({
            "reviewThread": review_thread::execute(
                &target,
                &review_thread_id,
                include_diff_hunk,
                include_details,
            )?,
        }),
        Action::ReviewThreads { include_resolved } => serde_json::json!({
            "reviewThreads": review_threads::execute(&target, include_resolved)?,
        }),
    };
    Ok(output::success(data))
}

fn positive_pr_number(value: &str) -> Option<String> {
    let value = super::target::positive_number(value)?;
    value.parse::<i32>().ok().map(|_| value)
}

fn usage(program: &str) -> String {
    format!(
        "usage: {program} pr [-h] {{overview,comments,reviews,review-threads,review-thread,checks}} ..."
    )
}

fn pr_argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(program, &usage(program), "pr", message)
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  {{overview,comments,reviews,review-threads,review-thread,checks}}\n    overview        read pull request state and summaries\n    comments        read pull request conversation comments\n    reviews         list pull request review submissions\n    review-threads  list review thread summaries\n    review-thread   read one review thread\n    checks          read individual checks and optional diagnostics\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program)
    );
    super::write_stdout(&text)
}
