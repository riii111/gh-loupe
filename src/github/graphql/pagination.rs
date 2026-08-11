use std::collections::HashSet;

use serde_json::Value;

use crate::error::{Exit, Result};

#[derive(Default)]
pub(super) struct CursorTracker {
    cursor: Option<String>,
    seen: HashSet<String>,
}

impl CursorTracker {
    pub(super) fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub(super) fn next(&mut self, connection: &Value) -> Result<Option<String>> {
        let page_info = value_at(connection, &["pageInfo"])?;
        let has_next = value_at(page_info, &["hasNextPage"])?
            .as_bool()
            .ok_or_else(|| {
                Exit::invalid_response("GitHub pageInfo.hasNextPage must be a boolean")
            })?;
        if !has_next {
            return Ok(None);
        }
        let cursor = value_at(page_info, &["endCursor"])?
            .as_str()
            .filter(|cursor| !cursor.is_empty())
            .ok_or_else(|| {
                Exit::invalid_response("GitHub pageInfo.endCursor must contain a cursor")
            })?;
        if !self.seen.insert(cursor.to_owned()) {
            return Err(Exit::invalid_response("GitHub pagination cursor repeated"));
        }
        self.cursor = Some(cursor.to_owned());
        Ok(self.cursor.clone())
    }
}

pub(super) fn nodes(connection: &Value) -> Result<&[Value]> {
    value_at(connection, &["nodes"])?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| Exit::invalid_response("GitHub connection nodes must be an array"))
}

pub(super) fn value_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value> {
    path.iter().try_fold(value, |value, key| {
        value
            .get(*key)
            .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {key}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(end_cursor: Value) -> Value {
        serde_json::json!({
            "nodes": [],
            "pageInfo": {
                "hasNextPage": true,
                "endCursor": end_cursor,
            },
        })
    }

    fn assert_invalid_response(error: Exit) {
        assert!(error.stderr_line().contains(r#""kind":"invalidResponse""#));
    }

    #[test]
    fn immediate_cursor_repeat_is_rejected() {
        let mut tracker = CursorTracker::default();
        assert_eq!(
            tracker.next(&page(Value::String("A".to_owned()))).ok(),
            Some(Some("A".to_owned()))
        );

        let error = tracker
            .next(&page(Value::String("A".to_owned())))
            .expect_err("repeated cursor must fail");
        assert_invalid_response(error);
    }

    #[test]
    fn non_adjacent_cursor_cycle_is_rejected() {
        let mut tracker = CursorTracker::default();
        for cursor in ["A", "B"] {
            assert!(
                tracker
                    .next(&page(Value::String(cursor.to_owned())))
                    .is_ok()
            );
        }

        let error = tracker
            .next(&page(Value::String("A".to_owned())))
            .expect_err("cursor cycle must fail");
        assert_invalid_response(error);
    }

    #[test]
    fn malformed_next_cursor_is_rejected() {
        let mut tracker = CursorTracker::default();
        for cursor in [Value::Null, Value::String(String::new()), Value::from(42)] {
            let error = tracker
                .next(&page(cursor))
                .expect_err("malformed cursor must fail");
            assert_invalid_response(error);
        }
    }
}
