mod actions_log;
mod diagnostics;
mod output;
mod validation;

use serde_json::{Map, Value};

use crate::error::{Exit, Result};
use crate::github;
use crate::model::{CheckDiagnosticsOptions, Target};

use self::diagnostics::{collect_diagnostics, diagnostic_deadline};
use self::output::{Check, compare_checks};
use self::validation::{validate_check, validate_check_contexts};

pub fn execute(target: &Target, required: bool, options: CheckDiagnosticsOptions) -> Result<Value> {
    let diagnostics_requested = options.failed_diagnostics || options.include_failed_logs;
    let timeout_message = format!(
        "failed check diagnostics timed out after {} seconds",
        options.timeout_seconds
    );
    let deadline = diagnostic_deadline(options.timeout_seconds).ok_or_else(|| Exit {
        message: format!(
            "argument --timeout: {} seconds cannot be represented as a diagnostic deadline",
            options.timeout_seconds
        ),
        code: 2,
    })?;
    let checks = if diagnostics_requested {
        let (head_oid, contexts) =
            github::graphql::pull_request_check_contexts(target, deadline, &timeout_message)?;
        let mut checks = validate_check_contexts(&contexts, required)?;
        checks.sort_by(compare_checks);
        collect_diagnostics(
            target,
            &mut checks,
            options,
            &head_oid,
            deadline,
            &timeout_message,
        )?;
        checks
    } else {
        let response = github::checks::checks(target, required)?;
        let values = response
            .as_array()
            .ok_or_else(|| Exit::invalid_response("GitHub returned an invalid checks response"))?;
        let mut checks = values
            .iter()
            .map(validate_check)
            .collect::<Result<Vec<_>>>()?;
        checks.sort_by(compare_checks);
        checks
    };

    let mut result = Map::new();
    result.insert(
        "checks".to_owned(),
        Value::Array(checks.into_iter().map(Check::into_value).collect()),
    );
    Ok(Value::Object(result))
}
