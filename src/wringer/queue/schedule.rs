use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::profile::persona::PersonaSchedule;

/// A parsed active-hours window: (start, end) in naive local time.
pub(super) struct ActiveWindow {
    pub(super) start: NaiveTime,
    pub(super) end: NaiveTime,
    /// Whether the window wraps midnight (e.g., "22:00-02:00").
    pub(super) wraps: bool,
}

pub(super) fn parse_active_windows(windows: &[String], _tz: Tz) -> Vec<ActiveWindow> {
    windows
        .iter()
        .filter_map(|s| {
            let (start_str, end_str) = s.split_once('-')?;
            let start = parse_naive_time(start_str)?;
            let end = parse_naive_time(end_str)?;
            let wraps = end <= start;
            Some(ActiveWindow { start, end, wraps })
        })
        .collect()
}

fn parse_naive_time(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    let hour: u32 = h.parse().ok()?;
    let minute: u32 = m.parse().ok()?;
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// Find the next moment >= `from` that falls inside any of the persona's active
pub(super) fn next_active_time(
    from: DateTime<Utc>,
    windows: &[ActiveWindow],
    tz: Tz,
) -> DateTime<Utc> {
    if windows.is_empty() {
        return from + Duration::hours(1);
    }

    let local = from.with_timezone(&tz);
    let local_time = local.time();

    for window in windows {
        if time_in_window(local_time, window) {
            return from;
        }
    }

    // Find the nearest upcoming window start.
    let mut best: Option<DateTime<Utc>> = None;
    for window in windows {
        let candidate_local = local.date_naive().and_time(window.start);
        let candidate_utc = tz
            .from_local_datetime(&candidate_local)
            .single()
            .map(|dt| dt.with_timezone(&Utc));

        if let Some(candidate) = candidate_utc {
            let candidate_adjusted = if candidate <= from {
                candidate + Duration::days(1)
            } else {
                candidate
            };

            if best.is_none_or(|b| candidate_adjusted < b) {
                best = Some(candidate_adjusted);
            }
        }
    }

    best.unwrap_or_else(|| from + Duration::hours(1))
}

fn time_in_window(t: NaiveTime, window: &ActiveWindow) -> bool {
    if window.wraps {
        t >= window.start || t < window.end
    } else {
        t >= window.start && t < window.end
    }
}

/// Compute a jitter duration in minutes based on persona session variance.
#[expect(
    clippy::cast_possible_truncation,
    reason = "jitter is bounded by session minutes, fits i64"
)]
pub(super) fn jitter_minutes(schedule: &PersonaSchedule) -> Duration {
    let avg = i64::from(schedule.avg_commits_per_session).max(1);
    // Spread commits roughly evenly across ~2 hour sessions.
    let session_minutes: i64 = 120;
    let per_commit_minutes = session_minutes / avg;
    // Simple deterministic jitter: vary by ±25% of per-commit interval.
    let variance = (f64::from(schedule.session_variance)
        * f64::from(i32::try_from(per_commit_minutes).unwrap_or(15)))
    .round() as i64;
    let jitter = variance.max(1);
    Duration::minutes(per_commit_minutes + jitter)
}

/// Advance cursor by `interval`, staying within active windows where possible.
pub(super) fn advance_cursor(
    cursor: DateTime<Utc>,
    interval: Duration,
    windows: &[ActiveWindow],
    tz: Tz,
) -> DateTime<Utc> {
    let next = cursor + interval;
    next_active_time(next, windows, tz)
}
