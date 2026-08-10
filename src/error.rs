use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "used by follow-up subcommands with schema v1 errors"
)]
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

    pub fn github_cli(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::GitHubCli,
            message: message.into(),
            retryable: false,
            retry_after_seconds: None,
        }
    }

    pub fn from_cli_failure(stderr: &[u8]) -> Self {
        let message = String::from_utf8_lossy(stderr).trim().to_owned();
        let message = if message.is_empty() {
            "GitHub CLI failed without an error message".to_owned()
        } else {
            message
        };
        let lower = message.to_ascii_lowercase();
        let (kind, retryable) = if contains_any(
            &lower,
            &[
                "not logged in",
                "authentication",
                "bad credentials",
                "http 401",
            ],
        ) {
            (ErrorKind::Authentication, false)
        } else if lower.contains("rate limit") {
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
                "could not resolve to a pull request",
            ],
        ) {
            (ErrorKind::NotFound, false)
        } else if contains_any(&lower, &["timed out", "timeout"]) {
            (ErrorKind::Timeout, true)
        } else if contains_any(
            &lower,
            &[
                "network",
                "connection reset",
                "connection refused",
                "could not resolve host",
                "temporary failure",
                "tls handshake",
            ],
        ) {
            (ErrorKind::Network, true)
        } else {
            (ErrorKind::GitHubCli, false)
        };
        Self {
            kind,
            message,
            retryable,
            retry_after_seconds: None,
        }
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
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

    pub fn child(code: i32, stderr: &[u8]) -> Self {
        Self {
            message: Some(String::from_utf8_lossy(stderr).trim_end().to_owned()),
            code,
        }
    }

    #[allow(
        dead_code,
        reason = "used by follow-up subcommands with schema v1 errors"
    )]
    pub fn runtime(error: &RuntimeError, code: i32) -> Self {
        Self {
            message: Some(
                serde_json::to_string(&error.json())
                    .expect("runtime error values are always serializable"),
            ),
            code,
        }
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
}
