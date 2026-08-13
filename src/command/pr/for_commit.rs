use crate::error::{Exit, Result};
use crate::output;
use crate::usecase;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

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
    let Some(sha) = positionals.next() else {
        return Err(argument_error(
            program,
            "the following arguments are required: sha",
        ));
    };
    if !is_commit_sha(&sha) {
        return Err(argument_error(
            program,
            "sha must be a hexadecimal commit SHA of 7 to 40 characters",
        ));
    }
    super::super::unrecognized_args(program, argument_error, &parsed.unrecognized)?;
    Ok(super::super::Args {
        action: super::super::Action::Pr(super::Action::ForCommit { limit }),
        target: sha,
        repo: parsed.repo,
        compact: parsed.compact,
        program: program.to_owned(),
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

fn is_commit_sha(value: &str) -> bool {
    (7..=40).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn execute(
    sha: &str,
    repo: Option<String>,
    limit: usize,
    program: &str,
) -> Result<serde_json::Value> {
    let repository = super::super::target::resolve_repo(repo, argument_error, program)?;
    Ok(output::success(usecase::search::for_commit(
        &repository,
        sha,
        limit,
    )?))
}

pub(super) fn argument_error(program: &str, message: &str) -> Exit {
    super::super::argument_error(
        program,
        &format!("usage: {program} pr for-commit [-h] [--repo REPO] [--compact] [--limit N] sha"),
        "pr for-commit",
        message,
    )
}

fn print_help(program: &str) -> Result<()> {
    super::super::write_stdout(&format!(
        "usage: {program} pr for-commit [-h] [--repo REPO] [--compact] [--limit N] sha\n\npositional arguments:\n  sha          hexadecimal commit SHA, 7 to 40 characters\n\noptions:\n  -h, --help   show this help message and exit\n  --repo REPO  OWNER/REPO; inferred from cwd when omitted\n  --limit N    return at most N pull requests, from 1 through 100 (default: 20)\n  --compact    emit one-line JSON\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_full_and_practical_short_sha() {
        assert!(is_commit_sha("0123456"));
        assert!(is_commit_sha("0123456789abcdef0123456789abcdef01234567"));
    }

    #[test]
    fn rejects_refs_and_ambiguous_sha_values() {
        for value in [
            "main",
            "012345",
            "0123456789abcdefg",
            "0123456789abcdef0123456789abcdef012345678",
        ] {
            assert!(!is_commit_sha(value));
        }
    }
}
