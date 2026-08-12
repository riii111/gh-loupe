use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};

pub(super) struct Check {
    pub(super) name: String,
    pub(super) state: String,
    pub(super) bucket: CheckBucket,
    pub(super) link: Option<String>,
    pub(super) workflow: Option<String>,
    pub(super) started_at: Option<String>,
    pub(super) completed_at: Option<String>,
    pub(super) check_run_id: Option<u64>,
    pub(super) annotations: Option<Vec<Annotation>>,
    pub(super) log: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

impl CheckBucket {
    pub(super) fn from_cli(value: &str) -> Result<Self> {
        match value {
            "pass" => Ok(Self::Pass),
            "fail" => Ok(Self::Fail),
            "pending" => Ok(Self::Pending),
            "skipping" => Ok(Self::Skipping),
            "cancel" => Ok(Self::Cancel),
            _ => Err(Exit::invalid_response(
                "GitHub returned an unknown check bucket",
            )),
        }
    }

    pub(super) fn from_state(state: &str) -> Result<Self> {
        match state {
            "SUCCESS" => Ok(Self::Pass),
            "SKIPPED" | "NEUTRAL" => Ok(Self::Skipping),
            "ERROR" | "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" => Ok(Self::Fail),
            "CANCELLED" => Ok(Self::Cancel),
            "EXPECTED" | "REQUESTED" | "WAITING" | "QUEUED" | "PENDING" | "IN_PROGRESS"
            | "STALE" | "STARTUP_FAILURE" => Ok(Self::Pending),
            _ => Err(Exit::invalid_response(
                "GitHub returned an unknown check state",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Pending => "pending",
            Self::Skipping => "skipping",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug)]
pub(super) struct Annotation {
    pub(super) path: String,
    pub(super) start_line: u64,
    pub(super) end_line: u64,
    pub(super) annotation_level: String,
    pub(super) title: Option<String>,
    pub(super) message: String,
}

impl Annotation {
    pub(super) fn into_value(self) -> Value {
        Value::Object(Map::from_iter([
            ("path".to_owned(), Value::String(self.path)),
            ("startLine".to_owned(), Value::from(self.start_line)),
            ("endLine".to_owned(), Value::from(self.end_line)),
            (
                "annotationLevel".to_owned(),
                Value::String(self.annotation_level),
            ),
            (
                "title".to_owned(),
                self.title.map_or(Value::Null, Value::String),
            ),
            ("message".to_owned(), Value::String(self.message)),
        ]))
    }
}

impl Check {
    pub(super) fn into_value(self) -> Value {
        let mut value = Map::from_iter([
            ("name".to_owned(), Value::String(self.name)),
            ("state".to_owned(), Value::String(self.state)),
            (
                "bucket".to_owned(),
                Value::String(self.bucket.as_str().to_owned()),
            ),
            (
                "link".to_owned(),
                self.link.map_or(Value::Null, Value::String),
            ),
            (
                "workflow".to_owned(),
                self.workflow.map_or(Value::Null, Value::String),
            ),
            (
                "startedAt".to_owned(),
                self.started_at.map_or(Value::Null, Value::String),
            ),
            (
                "completedAt".to_owned(),
                self.completed_at.map_or(Value::Null, Value::String),
            ),
        ]);
        if let Some(annotations) = self.annotations {
            value.insert(
                "annotations".to_owned(),
                Value::Array(
                    annotations
                        .into_iter()
                        .map(Annotation::into_value)
                        .collect(),
                ),
            );
        }
        if let Some(log) = self.log {
            value.insert("log".to_owned(), log);
        }
        Value::Object(value)
    }
}

pub(super) fn compare_checks(left: &Check, right: &Check) -> Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.link.cmp(&right.link))
        .then_with(|| right.started_at.cmp(&left.started_at))
        .then_with(|| left.check_run_id.cmp(&right.check_run_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_bucket_maps_all_known_states() {
        assert_eq!(bucket("SUCCESS"), CheckBucket::Pass);
        assert_eq!(bucket("SKIPPED"), CheckBucket::Skipping);
        assert_eq!(bucket("NEUTRAL"), CheckBucket::Skipping);
        assert_eq!(bucket("ERROR"), CheckBucket::Fail);
        assert_eq!(bucket("FAILURE"), CheckBucket::Fail);
        assert_eq!(bucket("TIMED_OUT"), CheckBucket::Fail);
        assert_eq!(bucket("ACTION_REQUIRED"), CheckBucket::Fail);
        assert_eq!(bucket("CANCELLED"), CheckBucket::Cancel);
        assert_eq!(bucket("EXPECTED"), CheckBucket::Pending);
        assert_eq!(bucket("REQUESTED"), CheckBucket::Pending);
        assert_eq!(bucket("WAITING"), CheckBucket::Pending);
        assert_eq!(bucket("QUEUED"), CheckBucket::Pending);
        assert_eq!(bucket("PENDING"), CheckBucket::Pending);
        assert_eq!(bucket("IN_PROGRESS"), CheckBucket::Pending);
        assert_eq!(bucket("STALE"), CheckBucket::Pending);
        assert_eq!(bucket("STARTUP_FAILURE"), CheckBucket::Pending);
        assert!(CheckBucket::from_state("UNKNOWN").is_err());
    }

    fn bucket(state: &str) -> CheckBucket {
        CheckBucket::from_state(state).unwrap_or_else(|_| panic!("known check state: {state}"))
    }
}
