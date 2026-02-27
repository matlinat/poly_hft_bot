use chrono::{DateTime, Duration, Timelike, Utc};

/// Length of a Polymarket short-term round in minutes.
pub const ROUND_MINUTES: i64 = 15;

/// Returns the start timestamp of the current 15-minute round for a given time.
pub fn round_start(ts: DateTime<Utc>) -> DateTime<Utc> {
    let minutes = ts.minute() as i64;
    let bucket = minutes - (minutes % ROUND_MINUTES);
    ts.date_naive()
        .and_hms_opt(ts.hour(), bucket as u32, 0)
        .unwrap()
        .and_utc()
}

/// Returns the end timestamp of the current 15-minute round.
pub fn round_end(ts: DateTime<Utc>) -> DateTime<Utc> {
    round_start(ts) + Duration::minutes(ROUND_MINUTES)
}

/// Returns seconds remaining in the current round.
pub fn seconds_remaining(ts: DateTime<Utc>) -> i64 {
    let end = round_end(ts);
    (end - ts).num_seconds().max(0)
}

/// Returns true if `now` is within the first `window_min` minutes of the round.
pub fn within_leg1_window(now: DateTime<Utc>, window_min: u64) -> bool {
    let start = round_start(now);
    let elapsed = (now - start).num_minutes();
    elapsed >= 0 && elapsed < window_min as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_boundaries() {
        let ts = DateTime::parse_from_rfc3339("2024-01-01T12:07:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let start = round_start(ts);
        let end = round_end(ts);
        assert_eq!(start.to_rfc3339(), "2024-01-01T12:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2024-01-01T12:15:00+00:00");
        assert_eq!(seconds_remaining(ts), 7 * 60 + 30);
    }

    #[test]
    fn test_within_leg1() {
        let ts = DateTime::parse_from_rfc3339("2024-01-01T12:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(within_leg1_window(ts, 2));
        let ts_late = DateTime::parse_from_rfc3339("2024-01-01T12:03:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!within_leg1_window(ts_late, 2));
    }
}

