use crate::error::{Exit, Result};
use crate::output;
use crate::usecase;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Issues { limit: usize },
    PullRequests { limit: usize },
}

pub(super) fn parse<I>(program: &str, mut remaining: I) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let Some(subcommand) = remaining.next() else {
        return Err(search_argument_error(
            program,
            "the following arguments are required: subcommand",
        ));
    };
    match subcommand.as_str() {
        "issues" => parse_args(program, remaining, ActionKind::Issues),
        "prs" => parse_args(program, remaining, ActionKind::PullRequests),
        "-h" | "--help" => {
            print_help(program)?;
            std::process::exit(0);
        }
        _ => Err(search_argument_error(
            program,
            &format!(
                "argument subcommand: invalid choice: '{subcommand}' (choose from 'issues', 'prs')"
            ),
        )),
    }
}

#[derive(Clone, Copy)]
enum ActionKind {
    Issues,
    PullRequests,
}

fn parse_args<I>(program: &str, values: I, kind: ActionKind) -> Result<super::Args>
where
    I: Iterator<Item = String>,
{
    let mut limit = DEFAULT_LIMIT;
    let print_help: fn(&str) -> Result<()> = match kind {
        ActionKind::Issues => print_issues_help,
        ActionKind::PullRequests => print_pull_requests_help,
    };
    let parsed = super::parse_subcommand_args(
        program,
        values,
        1,
        argument_error,
        print_help,
        |option, values| {
            if let Some(value) = super::exact_long_option_value(option, "--limit") {
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
    let Some(query) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: query",
        ));
    };
    if query.trim().is_empty() {
        return Err(argument_error(program, "query must not be empty"));
    }
    if contains_scope_or_type_qualifier(&query) {
        return Err(argument_error(
            program,
            "query must not contain repository or issue type qualifiers",
        ));
    }
    super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    let action = match kind {
        ActionKind::Issues => Action::Issues { limit },
        ActionKind::PullRequests => Action::PullRequests { limit },
    };
    Ok(super::Args {
        action: super::Action::Search(action),
        target: query,
        repo: parsed.repo,
        compact: parsed.compact,
        program: program.to_owned(),
    })
}

fn contains_scope_or_type_qualifier(query: &str) -> bool {
    query.split_whitespace().any(|token| {
        let token = token
            .trim_matches(|character: char| matches!(character, '(' | ')' | ','))
            .to_ascii_lowercase();
        let token = token.strip_prefix('-').unwrap_or(&token);
        token.starts_with("repo:")
            || token.starts_with("org:")
            || token.starts_with("user:")
            || matches!(token, "is:issue" | "is:pr")
    })
}

fn parse_limit(value: &str, program: &str) -> Result<usize> {
    let limit = value.parse::<usize>().ok();
    if !limit.is_some_and(|limit| (1..=MAX_LIMIT).contains(&limit)) {
        return Err(argument_error(
            program,
            "argument --limit: must be between 1 and 100",
        ));
    }
    Ok(limit.expect("limit was validated"))
}

pub(super) fn execute(
    action: Action,
    query: &str,
    repo: Option<String>,
    program: &str,
) -> Result<serde_json::Value> {
    let repository = super::target::resolve_repo(repo, argument_error, program)?;
    let data = match action {
        Action::Issues { limit } => usecase::search::issues(&repository, query, limit)?,
        Action::PullRequests { limit } => {
            usecase::search::pull_requests(&repository, query, limit)?
        }
    };
    Ok(output::success(data))
}

fn usage(program: &str) -> String {
    format!("usage: {program} search [-h] {{issues,prs}} ...")
}

fn search_argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(program, &usage(program), "search", message)
}

fn argument_error(program: &str, message: &str) -> Exit {
    super::argument_error(
        program,
        &format!(
            "usage: {program} search {{issues,prs}} [-h] [--repo REPO] [--compact] [--limit N] query"
        ),
        "search",
        message,
    )
}

fn print_help(program: &str) -> Result<()> {
    let text = format!(
        "{}\n\npositional arguments:\n  {{issues,prs}}\n    issues    search Issues in one repository\n    prs       search pull requests in one repository\n\noptions:\n  -h, --help  show this help message and exit\n",
        usage(program)
    );
    super::write_stdout(&text)
}

fn print_issues_help(program: &str) -> Result<()> {
    print_subcommand_help(program, "issues", "search Issues in one repository")
}

fn print_pull_requests_help(program: &str) -> Result<()> {
    print_subcommand_help(program, "prs", "search pull requests in one repository")
}

fn print_subcommand_help(program: &str, name: &str, description: &str) -> Result<()> {
    super::write_stdout(&format!(
        "usage: {program} search {name} [-h] [--repo REPO] [--compact] [--limit N] query\n\npositional arguments:\n  query       GitHub search keywords\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --limit N    return at most N items, from 1 through 100 (default: 20)\n  --compact    emit one-line JSON\n\n{description}\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_default_and_boundary_limits() {
        for value in ["1", "20", "100"] {
            assert!(parse_limit(value, "gh-loupe").is_ok());
        }
    }

    #[test]
    fn rejects_limits_outside_the_public_range() {
        for value in ["0", "101", "not-a-number"] {
            assert!(parse_limit(value, "gh-loupe").is_err());
        }
    }

    #[test]
    fn rejects_scope_and_issue_type_qualifiers() {
        for query in [
            "repo:other/repo keyword",
            "org:other keyword",
            "user:other keyword",
            "is:issue keyword",
            "is:pr keyword",
            "-repo:owner/repo keyword",
        ] {
            assert!(contains_scope_or_type_qualifier(query));
        }
        assert!(!contains_scope_or_type_qualifier("state:open keyword"));
    }
}
