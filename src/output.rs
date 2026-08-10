use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

#[allow(
    dead_code,
    reason = "used by follow-up subcommands with schema v1 output"
)]
pub fn success(data: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "observedAt": utc_rfc3339(SystemTime::now()),
        "data": data,
    })
}

fn utc_rfc3339(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .expect("the system clock must be after the Unix epoch")
        .as_secs();
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_date(days);
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_date(days_since_epoch: u64) -> (i64, u64, u64) {
    let days = i64::try_from(days_since_epoch).expect("system time must fit in i64") + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u64, day as u64)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn envelope_uses_schema_v1_and_utc_completion_time() {
        let envelope = success(json!({"value": 42}));

        assert_eq!(envelope["schemaVersion"], 1);
        assert_eq!(envelope["data"], json!({"value": 42}));
        let observed_at = envelope["observedAt"].as_str().expect("observedAt string");
        assert_eq!(observed_at.len(), 20);
        assert!(observed_at.ends_with('Z'));
    }

    #[test]
    fn utc_timestamp_handles_epoch_and_leap_day() {
        assert_eq!(utc_rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_rfc3339(UNIX_EPOCH + Duration::from_secs(1_709_251_199)),
            "2024-02-29T23:59:59Z"
        );
    }
}
