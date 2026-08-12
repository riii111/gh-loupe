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

pub(super) fn take_value_at(mut value: Value, path: &[&str]) -> Result<Value> {
    for key in path {
        value = value
            .as_object_mut()
            .and_then(|object| object.shift_remove(*key))
            .ok_or_else(|| Exit::invalid_response(format!("GitHub response omitted {key}")))?;
    }
    Ok(value)
}

pub(super) fn take_nodes(connection: &mut Value) -> Result<Vec<Value>> {
    let nodes = connection
        .as_object_mut()
        .and_then(|object| object.shift_remove("nodes"))
        .ok_or_else(|| Exit::invalid_response("GitHub response omitted nodes"))?;
    match nodes {
        Value::Array(nodes) => Ok(nodes),
        _ => Err(Exit::invalid_response(
            "GitHub connection nodes must be an array",
        )),
    }
}

pub(super) fn append_connection_pages<F>(connection: &mut Value, mut fetch_page: F) -> Result<()>
where
    F: FnMut(String) -> Result<Value>,
{
    let mut cursor_tracker = CursorTracker::default();
    while let Some(cursor) = cursor_tracker.next(connection)? {
        let mut page = fetch_page(cursor)?;
        let new_nodes = take_nodes(&mut page)?;
        connection
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| Exit::invalid_response("GitHub connection nodes must be an array"))?
            .extend(new_nodes);
        connection
            .as_object_mut()
            .ok_or_else(|| Exit::invalid_response("GitHub comments must be an object"))?
            .insert("pageInfo".to_owned(), take_value_at(page, &["pageInfo"])?);
    }
    Ok(())
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
