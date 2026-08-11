use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Authorization,
    NotFound,
    RateLimited,
    Timeout,
    Network,
    GitHubCli,
    InvalidResponse,
}

impl ErrorKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Authorization => "authorization",
            Self::NotFound => "notFound",
            Self::RateLimited => "rateLimited",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::GitHubCli => "githubCli",
            Self::InvalidResponse => "invalidResponse",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub kind: ErrorKind,
    pub message: String,
    pub retryable: bool,
    pub retry_after_seconds: Option<u64>,
}

impl RuntimeError {
    pub fn json(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "error": {
                "kind": self.kind.as_str(),
                "message": self.message,
                "retryable": self.retryable,
                "retryAfterSeconds": self.retry_after_seconds,
            }
        })
    }

    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidResponse,
            message: message.into(),
            retryable: false,
            retry_after_seconds: None,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NotFound,
            message: message.into(),
            retryable: false,
            retry_after_seconds: None,
        }
    }

    pub fn from_cli_failure(stderr: &[u8]) -> Self {
        Self::classify_cli_failure(None, stderr)
    }

    pub(crate) fn from_cli_process_failure(code: i32, stderr: &[u8]) -> Self {
        Self::classify_cli_failure(Some(code), stderr)
    }

    fn classify_cli_failure(code: Option<i32>, stderr: &[u8]) -> Self {
        let message = String::from_utf8_lossy(stderr).trim().to_owned();
        let message = if message.is_empty() {
            code.map_or_else(
                || "GitHub CLI failed without an error message".to_owned(),
                |code| format!("GitHub CLI exited with status {code}"),
            )
        } else {
            message
        };
        let lower = message.to_ascii_lowercase();
        let (kind, retryable) = if code == Some(4)
            || contains_any(
                &lower,
                &[
                    "gh auth login",
                    "not logged in",
                    "authentication",
                    "bad credentials",
                    "http 401",
                ],
            ) {
            (ErrorKind::Authentication, false)
        } else if contains_words(&lower, &["rate", "limit"])
            || contains_words(&lower, &["secondary", "rate"])
        {
            (ErrorKind::RateLimited, true)
        } else if contains_any(
            &lower,
            &["forbidden", "http 403", "resource not accessible"],
        ) {
            (ErrorKind::Authorization, false)
        } else if contains_any(
            &lower,
            &[
                "not found",
                "http 404",
                "could not resolve to a pullrequest",
                "could not resolve to a pull request",
                "could not resolve to a repository",
            ],
        ) {
            (ErrorKind::NotFound, false)
        } else if contains_any(
            &lower,
            &[
                "error connecting to ",
                "check your internet connection",
                "connection reset",
                "connection refused",
                "could not resolve host",
                "dial tcp",
                "no such host",
                "temporary failure",
                "tls handshake",
            ],
        ) || contains_word(&lower, "network")
        {
            (ErrorKind::Network, true)
        } else if contains_any(&lower, &["timed out", "timeout"]) {
            (ErrorKind::Timeout, true)
        } else {
            (ErrorKind::GitHubCli, false)
        };
        let retry_after_seconds = (kind == ErrorKind::RateLimited)
            .then(|| parse_retry_after_seconds(&lower))
            .flatten();
        Self {
            kind,
            message,
            retryable,
            retry_after_seconds,
        }
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn contains_word(value: &str, word: &str) -> bool {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| part == word)
}

fn contains_words(value: &str, words: &[&str]) -> bool {
    let parts = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts
        .windows(words.len())
        .any(|window| window.iter().zip(words).all(|(part, word)| part == word))
}

fn parse_retry_after_seconds(message: &str) -> Option<u64> {
    ["retry-after:", "retry after"]
        .into_iter()
        .find_map(|marker| {
            let remainder = message.split_once(marker)?.1.trim_start();
            let digits = remainder
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
        })
}

pub struct Exit {
    pub message: Option<String>,
    pub code: i32,
}

pub type Result<T> = std::result::Result<T, Exit>;

impl Exit {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            code: 1,
        }
    }

    pub fn runtime(error: &RuntimeError, code: i32) -> Self {
        Self {
            message: Some(
                serde_json::to_string(&error.json())
                    .expect("runtime error values are always serializable"),
            ),
            code,
        }
    }

    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::runtime(
            &RuntimeError {
                kind: ErrorKind::InvalidResponse,
                message: message.into(),
                retryable: false,
                retry_after_seconds: None,
            },
            1,
        )
    }

    pub fn stderr_line(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_runtime_error_has_schema_v1_shape() {
        let error = RuntimeError {
            kind: ErrorKind::RateLimited,
            message: "API rate limit exceeded\ntry later".to_owned(),
            retryable: true,
            retry_after_seconds: Some(30),
        };
        let exit = Exit::runtime(&error, 1);

        assert_eq!(
            exit.stderr_line(),
            Some(
                r#"{"schemaVersion":1,"error":{"kind":"rateLimited","message":"API rate limit exceeded\ntry later","retryable":true,"retryAfterSeconds":30}}"#
            )
        );
        assert_eq!(
            exit.stderr_line().expect("structured line").lines().count(),
            1
        );
    }

    #[test]
    fn every_runtime_error_kind_has_the_documented_spelling() {
        let kinds = [
            (ErrorKind::Authentication, "authentication"),
            (ErrorKind::Authorization, "authorization"),
            (ErrorKind::NotFound, "notFound"),
            (ErrorKind::RateLimited, "rateLimited"),
            (ErrorKind::Timeout, "timeout"),
            (ErrorKind::Network, "network"),
            (ErrorKind::GitHubCli, "githubCli"),
            (ErrorKind::InvalidResponse, "invalidResponse"),
        ];

        for (kind, expected) in kinds {
            assert_eq!(kind.as_str(), expected);
        }
    }

    #[test]
    fn cli_failures_use_the_shared_classification_table() {
        let cases = [
            (4, b"".as_slice(), ErrorKind::Authentication, false, None),
            (
                1,
                b"gh auth login required".as_slice(),
                ErrorKind::Authentication,
                false,
                None,
            ),
            (
                1,
                b"secondary rate limit; retry-after: 45".as_slice(),
                ErrorKind::RateLimited,
                true,
                Some(45),
            ),
            (
                1,
                b"resource not accessible by integration".as_slice(),
                ErrorKind::Authorization,
                false,
                None,
            ),
            (
                1,
                b"could not resolve to a PullRequest".as_slice(),
                ErrorKind::NotFound,
                false,
                None,
            ),
            (
                1,
                b"request timed out".as_slice(),
                ErrorKind::Timeout,
                true,
                None,
            ),
            (
                1,
                b"dial tcp: lookup api.github.com: no such host".as_slice(),
                ErrorKind::Network,
                true,
                None,
            ),
            (
                1,
                b"unexpected gh output".as_slice(),
                ErrorKind::GitHubCli,
                false,
                None,
            ),
        ];

        for (code, stderr, kind, retryable, retry_after_seconds) in cases {
            let error = RuntimeError::from_cli_process_failure(code, stderr);
            assert_eq!(
                (error.kind, error.retryable, error.retry_after_seconds),
                (kind, retryable, retry_after_seconds,)
            );
        }
    }

    #[test]
    fn cli_failure_markers_do_not_match_unrelated_substrings() {
        for stderr in [
            b"networking support is disabled".as_slice(),
            b"the not-foundish value was rejected".as_slice(),
            b"rate limiting is configured locally".as_slice(),
        ] {
            let error = RuntimeError::from_cli_process_failure(1, stderr);
            assert_eq!(error.kind, ErrorKind::GitHubCli);
            assert!(!error.retryable);
        }
    }
}
