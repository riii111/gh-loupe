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
