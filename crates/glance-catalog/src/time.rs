//! Relative-time rendering, ported from `weave/apps/fleet-retro/src/render.rs`
//! (itself ported from bridge.py's `relative_time`, 211 proven live renders
//! there) so every catalog consumer gets "16h ago" instead of a raw ISO
//! string as visible text for free (designer critique, aesthetic-927,
//! finding #3) -- one shared helper instead of a third hand-rolled copy the
//! next report kind would otherwise write.

use chrono::{DateTime, Utc};

/// Falls back to the raw string when it fails to parse -- degraded, not a
/// crash. `now` should be the report's own `generated_at`, not the wall
/// clock, so a statically rendered page's "how long ago" stays a pure
/// function of its own data rather than drifting every time it's reloaded.
pub fn relative_time(raw: &str, now: DateTime<Utc>) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        return raw.to_string();
    };
    let dt = parsed.with_timezone(&Utc);
    let delta = (now - dt).num_seconds();
    if delta < 60 {
        return "just now".to_string();
    }
    if delta < 3_600 {
        return format!("{}m ago", delta / 60);
    }
    if delta < 86_400 {
        return format!("{}h ago", delta / 3_600);
    }
    if delta < 7 * 86_400 {
        return format!("{}d ago", delta / 86_400);
    }
    dt.format("%b %-d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn renders_hours_ago() {
        let now = at("2026-07-05T21:00:00Z");
        assert_eq!(relative_time("2026-07-05T04:25:01Z", now), "16h ago");
    }

    #[test]
    fn renders_just_now_under_a_minute() {
        let now = Utc.with_ymd_and_hms(2026, 7, 5, 21, 0, 30).unwrap();
        assert_eq!(relative_time("2026-07-05T21:00:00Z", now), "just now");
    }

    #[test]
    fn falls_back_to_raw_string_on_parse_failure_instead_of_panicking() {
        let now = at("2026-07-05T21:00:00Z");
        assert_eq!(relative_time("not-a-timestamp", now), "not-a-timestamp");
    }
}
