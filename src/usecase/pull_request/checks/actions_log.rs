use serde_json::{Value, json};
use std::time::Instant;

use crate::error::{Exit, Result};
use crate::github;
use crate::model::Target;

use super::validation::{required_string, required_u64};

pub(super) const LOG_LINE_LIMIT: usize = 200;
pub(super) const LOG_BYTE_LIMIT: usize = 64 * 1024;
const UTF8_BOUNDARY_BYTES: usize = 4;

pub(super) fn collect_actions_log(
    target: &Target,
    link: Option<&str>,
    check_run_id: Option<u64>,
    head_oid: &str,
    max_bytes: usize,
    max_lines: usize,
    deadline: Instant,
    timeout_message: &str,
) -> Result<Value> {
    let Some(job_id) = link.and_then(|link| actions_job_id(target, link)) else {
        return Ok(Value::Null);
    };
    let job = github::checks::job(target, job_id, deadline, timeout_message)?;
    let job = job
        .as_object()
        .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid Actions job response"))?;
    let returned_id = required_u64(job, "id", "Actions job identifier")?;
    if returned_id != job_id {
        return Err(Exit::invalid_response(
            "GitHub returned a mismatched Actions job identifier",
        ));
    }
    let check_run_url = required_string(job, "check_run_url", "Actions job check run URL")?;
    let job_head_oid = required_string(job, "head_sha", "Actions job head SHA")?;
    let job_check_run_id = actions_check_run_id(target, check_run_url);
    if job_check_run_id != check_run_id || job_head_oid != head_oid {
        return Ok(Value::Null);
    }

    let bytes = github::checks::job_log(
        target,
        job_id,
        max_bytes,
        max_lines,
        deadline,
        timeout_message,
    )?;
    truncate_log(bytes)
}

fn actions_job_id(target: &Target, link: &str) -> Option<u64> {
    let path = strip_ascii_case_prefix(link, "https://github.com/")?;
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    if !repository_matches(target, owner, repository)
        || segments.next()? != "actions"
        || segments.next()? != "runs"
    {
        return None;
    }
    let run_id = segments.next()?;
    if segments.next()? != "job" {
        return None;
    }
    let job_id = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    run_id.parse::<u64>().ok()?;
    job_id.parse().ok()
}

fn actions_check_run_id(target: &Target, url: &str) -> Option<u64> {
    let path = strip_ascii_case_prefix(url, "https://api.github.com/repos/")?;
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    if !repository_matches(target, owner, repository) || segments.next()? != "check-runs" {
        return None;
    }
    let check_run_id = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    check_run_id.parse().ok()
}

fn repository_matches(target: &Target, owner: &str, repository: &str) -> bool {
    target
        .repository
        .split_once('/')
        .is_some_and(|(target_owner, target_repository)| {
            target_owner.eq_ignore_ascii_case(owner)
                && target_repository.eq_ignore_ascii_case(repository)
        })
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = value.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

fn truncate_log(log: github::checks::BoundedBytes) -> Result<Value> {
    let github::checks::BoundedBytes {
        bytes,
        total_bytes,
        total_newlines,
        valid_utf8,
    } = log;
    if !valid_utf8 {
        return Err(Exit::invalid_response(
            "GitHub returned a non-UTF-8 job log",
        ));
    }
    let byte_start = bytes.len().saturating_sub(LOG_BYTE_LIMIT);
    let mut start = byte_start;
    let mut text = std::str::from_utf8(&bytes[start..]);
    while let Err(error) = text {
        if error.valid_up_to() != 0
            || start >= byte_start.saturating_add(UTF8_BOUNDARY_BYTES)
            || start >= bytes.len()
        {
            return Err(Exit::invalid_response(
                "GitHub returned a non-UTF-8 job log",
            ));
        }
        start += 1;
        text = std::str::from_utf8(&bytes[start..]);
    }
    let text = text.expect("valid UTF-8 log after boundary adjustment");
    let omitted_bytes = total_bytes.saturating_sub(bytes.len() as u64) + start as u64;
    let omitted_lines = total_newlines.saturating_sub(newline_count(&bytes[start..]));
    Ok(json!({
        "text": text,
        "truncated": omitted_bytes > 0,
        "omittedLines": omitted_lines,
        "omittedBytes": omitted_bytes,
    }))
}

fn newline_count(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0_u64, |count, byte| count + u64::from(*byte == b'\n'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_applies_line_limit_before_byte_limit() {
        let long_line = "x".repeat(LOG_BYTE_LIMIT);
        let mut input = (0..201).map(|_| "old\n").collect::<String>();
        input.push_str(&long_line);
        input.push_str("tail");
        let total_bytes = input.len() as u64;
        let total_newlines = newline_count(input.as_bytes());

        let log = truncate_log(github::checks::BoundedBytes {
            bytes: input.into_bytes(),
            total_bytes,
            total_newlines,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("truncate log"));

        assert_eq!(
            log["text"].as_str().expect("log text").len(),
            LOG_BYTE_LIMIT
        );
        assert_eq!(log["omittedLines"], 201);
        assert_eq!(log["omittedBytes"], 808);
        assert_eq!(log["truncated"], true);
    }

    #[test]
    fn log_within_both_limits_reports_no_omissions() {
        let bytes = b"first\nsecond\n".to_vec();
        let log = truncate_log(github::checks::BoundedBytes {
            total_bytes: bytes.len() as u64,
            total_newlines: newline_count(&bytes),
            bytes,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("retain log"));

        assert_eq!(log["text"], "first\nsecond\n");
        assert_eq!(log["truncated"], false);
        assert_eq!(log["omittedLines"], 0);
        assert_eq!(log["omittedBytes"], 0);
    }

    #[test]
    fn multibyte_character_crossing_byte_limit_is_not_split() {
        let mut input = vec![b'x'; LOG_BYTE_LIMIT - 1];
        input.extend_from_slice("あ".as_bytes());
        input.extend(std::iter::repeat_n(b'x', LOG_BYTE_LIMIT - 7));
        input.extend_from_slice(b"tail\n");
        let total_bytes = input.len() as u64;
        let total_newlines = newline_count(&input);
        let retained_start = input.len() - (LOG_BYTE_LIMIT + UTF8_BOUNDARY_BYTES);
        let bytes = input[retained_start..].to_vec();

        let log = truncate_log(github::checks::BoundedBytes {
            bytes,
            total_bytes,
            total_newlines,
            valid_utf8: true,
        })
        .unwrap_or_else(|_| panic!("truncate UTF-8 log"));

        assert!(
            log["text"]
                .as_str()
                .is_some_and(|text| text.ends_with("tail\n"))
        );
        assert_eq!(log["omittedBytes"], 65_538);
        assert_eq!(log["omittedLines"], 0);
    }

    #[test]
    fn actions_job_requires_the_fixed_repository_url_shape() {
        let target = Target {
            repository: "owner/repo".to_owned(),
            number: "1".to_owned(),
        };

        assert_eq!(
            actions_job_id(
                &target,
                "https://github.com/owner/repo/actions/runs/10/job/20?pr=1"
            ),
            Some(20)
        );
        assert_eq!(
            actions_job_id(&target, "https://example.test/actions/runs/10/job/20"),
            None
        );
    }

    #[test]
    fn actions_urls_match_repository_names_case_insensitively() {
        let target = Target {
            repository: "Owner/Repo".to_owned(),
            number: "1".to_owned(),
        };

        assert_eq!(
            actions_job_id(
                &target,
                "https://github.com/owner/repo/actions/runs/10/job/20"
            ),
            Some(20)
        );
        assert_eq!(
            actions_check_run_id(
                &target,
                "https://api.github.com/repos/owner/repo/check-runs/100"
            ),
            Some(100)
        );
    }
}
