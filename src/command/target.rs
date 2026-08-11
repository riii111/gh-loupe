use crate::error::{Exit, Result};
use crate::github;
use crate::model::Target;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Pr,
    Issue,
}

pub(super) fn resolve_issue_target(target: &str, repo: Option<String>) -> Result<Target> {
    resolve_target(target, repo, Resource::Issue)
}

fn resolve_target(target: &str, repo: Option<String>, resource: Resource) -> Result<Target> {
    if let Some((url_repo, number)) = parse_url(target, resource) {
        if !is_repo(url_repo) {
            let name = resource_name(resource);
            return Err(Exit::message(format!(
                "{name} URL must contain a valid OWNER/REPO"
            )));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            let resource_name = resource_url_name(resource);
            return Err(Exit::message(format!(
                "--repo conflicts with the {resource_name} URL"
            )));
        }
        if let Some(number) = positive_number(number) {
            return Ok(Target {
                repository: url_repo.to_owned(),
                number,
            });
        }
    }

    let Some(number) = positive_number(target) else {
        let name = resource_name(resource);
        return Err(Exit::message(format!(
            "{name} must be a positive number or GitHub {name} URL"
        )));
    };
    let repository = resolve_repo(repo)?;
    Ok(Target { repository, number })
}

fn resolve_repo(repo: Option<String>) -> Result<String> {
    let repo = match repo {
        Some(repo) => repo,
        None => github::current_repository_runtime()?,
    };
    if !is_repo(&repo) {
        return Err(Exit::message("--repo must use OWNER/REPO format"));
    }
    Ok(repo)
}

pub(super) fn resolve_pr_subcommand_target(
    target: &str,
    repo: Option<String>,
    program: &str,
    argument_error: fn(&str, &str) -> Exit,
) -> Result<Target> {
    if let Some((url_repo, number)) = parse_url(target, Resource::Pr) {
        if !is_repo(url_repo) {
            return Err(argument_error(
                program,
                "pr URL must contain a valid OWNER/REPO",
            ));
        }
        if repo
            .as_ref()
            .is_some_and(|repo| !repo.eq_ignore_ascii_case(url_repo))
        {
            return Err(argument_error(
                program,
                "--repo conflicts with the pull request URL",
            ));
        }
        let Some(number) = positive_pr_number(number) else {
            return Err(argument_error(
                program,
                "pr must be a positive number within GitHub GraphQL Int range or GitHub pr URL",
            ));
        };
        return Ok(Target {
            repository: url_repo.to_owned(),
            number,
        });
    }

    let Some(number) = positive_pr_number(target) else {
        return Err(argument_error(
            program,
            "pr must be a positive number within GitHub GraphQL Int range or GitHub pr URL",
        ));
    };
    let repository = match repo {
        Some(repo) => repo,
        None => github::current_repository_runtime()?,
    };
    if !is_repo(&repository) {
        return Err(argument_error(program, "--repo must use OWNER/REPO format"));
    }
    Ok(Target { repository, number })
}

fn parse_url(target: &str, resource: Resource) -> Option<(&str, &str)> {
    let path = target.strip_prefix("https://github.com/")?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    let kind = segments.next()?;
    let number = segments.next()?;
    if segments.next().is_some()
        || kind
            != match resource {
                Resource::Pr => "pull",
                Resource::Issue => "issues",
            }
    {
        return None;
    }
    let repo_length = owner.len() + 1 + name.len();
    Some((&path[..repo_length], number))
}

fn positive_number(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = value.trim_start_matches('0');
    (!value.is_empty()).then(|| value.to_owned())
}

fn positive_pr_number(value: &str) -> Option<String> {
    let value = positive_number(value)?;
    value.parse::<i32>().ok().map(|_| value)
}

fn is_repo(value: &str) -> bool {
    let mut segments = value.split('/');
    let Some(owner) = segments.next() else {
        return false;
    };
    let Some(name) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && [owner, name].into_iter().all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        })
}

const fn resource_name(resource: Resource) -> &'static str {
    match resource {
        Resource::Pr => "pr",
        Resource::Issue => "issue",
    }
}

const fn resource_url_name(resource: Resource) -> &'static str {
    match resource {
        Resource::Pr => "pull request",
        Resource::Issue => "issue",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks_argument_error(program: &str, message: &str) -> Exit {
        Exit {
            message: Some(format!("{program} pr checks: error: {message}")),
            code: 2,
        }
    }

    fn overview_argument_error(program: &str, message: &str) -> Exit {
        Exit {
            message: Some(format!("{program} pr overview: error: {message}")),
            code: 2,
        }
    }

    #[test]
    fn common_pr_target_resolver_keeps_subcommand_specific_errors() {
        for (argument_error, subcommand) in [
            (checks_argument_error as fn(&str, &str) -> Exit, "checks"),
            (overview_argument_error, "overview"),
        ] {
            let result = resolve_pr_subcommand_target(
                "0",
                Some("owner/repo".to_owned()),
                "gh-read",
                argument_error,
            );
            let Err(error) = result else {
                panic!("expected a target error")
            };
            assert_eq!(error.code, 2);
            assert!(error.stderr_line().is_some_and(|message| {
                message.contains(&format!("gh-read pr {subcommand}: error:"))
            }));
        }
    }
}
