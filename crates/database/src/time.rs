//! Timestamp formatting.
//!
//! Every stored timestamp is RFC 3339 in UTC with second precision, e.g.
//! `2026-07-29T00:11:04Z`. Text, so it sorts correctly in SQLite and is
//! readable when inspecting the file by hand.
//!
//! Wall-clock time is used for storage and display only. Durations, timeouts
//! and rate limiting use monotonic clocks — see `docs/offline-mode.md` §6 —
//! so a machine resuming from sleep cannot extend a session.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as an RFC 3339 UTC string.
pub fn now() -> String {
    format_unix_seconds(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or(0),
    )
}

/// Format seconds since the Unix epoch as RFC 3339 UTC.
///
/// Implemented directly rather than pulled from a date library: the agent needs
/// exactly one format, and the civil-from-days conversion below is a well-known
/// algorithm that is cheaper to verify than an extra dependency is to audit.
pub fn format_unix_seconds(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Offset a stored timestamp by a number of seconds.
///
/// Parses back to a Unix instant rather than doing text arithmetic, so month
/// and year boundaries are handled by the same code that formats them.
pub fn add_seconds(timestamp: &str, seconds: i64) -> String {
    match parse_unix_seconds(timestamp) {
        Some(base) => format_unix_seconds(base.saturating_add(seconds)),
        // An unparsable stored timestamp should not silently become epoch-plus
        // an offset; returning the input unchanged keeps the anomaly visible.
        None => timestamp.to_string(),
    }
}

/// Parse an RFC 3339 UTC timestamp produced by [`format_unix_seconds`].
pub fn parse_unix_seconds(timestamp: &str) -> Option<i64> {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 20 || bytes.get(19) != Some(&b'Z') {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        timestamp.get(range)?.parse::<i64>().ok()
    };

    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Inverse of [`civil_from_days`].
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_position = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_position + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Howard Hinnant's `civil_from_days`, days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_unix_seconds(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix_seconds(1), "1970-01-01T00:00:01Z");
        assert_eq!(format_unix_seconds(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_unix_seconds(1_600_000_000), "2020-09-13T12:26:40Z");
    }

    #[test]
    fn handles_leap_days() {
        // 2020-02-29 was a leap day; 2100 is not a leap year.
        assert_eq!(format_unix_seconds(1_582_934_400), "2020-02-29T00:00:00Z");
        assert_eq!(format_unix_seconds(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn timestamps_sort_lexicographically() {
        // The whole reason for this format: text ordering must equal time
        // ordering, or every `ORDER BY created_at` is subtly wrong.
        let earlier = format_unix_seconds(1_600_000_000);
        let later = format_unix_seconds(1_700_000_000);
        assert!(earlier < later, "{earlier} should sort before {later}");
    }

    #[test]
    fn now_has_the_expected_shape() {
        let stamp = now();
        assert_eq!(stamp.len(), 20, "got {stamp}");
        assert!(stamp.ends_with('Z'), "got {stamp}");
        assert!(stamp.contains('T'), "got {stamp}");
        // Sanity: this project did not exist before 2020.
        assert!(stamp.as_str() > "2020-01-01T00:00:00Z", "got {stamp}");
    }
}
