// SPDX-License-Identifier: Apache-2.0
//! The time that a report carries. The core reads no clock: the command
//! injects the date of the report and the duration of the scan, or with
//! `--deterministic` fixed values, so that tests get the same bytes each run.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The `generated_at` of a deterministic report: the epoch (SPEC M4).
pub const DETERMINISTIC_GENERATED_AT: &str = "1970-01-01T00:00:00Z";

/// The last second that an RFC 3339 date writes with four digits for the year,
/// 9999-12-31T23:59:59Z.
const LAST_SECOND: u64 = 253_402_300_799;

/// Where a report takes its time from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    /// The clock of the system.
    System,
    /// The epoch, and no duration.
    Fixed,
}

impl Clock {
    /// The system clock, or when `deterministic` a fixed time.
    pub fn new(deterministic: bool) -> Self {
        if deterministic {
            Self::Fixed
        } else {
            Self::System
        }
    }

    /// The date of the report: an RFC 3339 date-time in UTC.
    pub fn generated_at(self) -> String {
        match self {
            Self::Fixed => DETERMINISTIC_GENERATED_AT.to_owned(),
            Self::System => rfc3339(SystemTime::now()),
        }
    }

    /// The milliseconds since `started`, none for a fixed clock.
    pub fn duration_ms(self, started: Instant) -> Option<u64> {
        match self {
            Self::Fixed => None,
            Self::System => Some(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        }
    }
}

/// `time` as an RFC 3339 date-time in UTC, to the second.
///
/// humantime writes the years 1970 to 9999 and panics outside them: a clock
/// that is set wrong gives the epoch, which is also the time of a fixed clock,
/// not a crash.
fn rfc3339(time: SystemTime) -> String {
    let writable = time
        .duration_since(UNIX_EPOCH)
        .is_ok_and(|elapsed| elapsed.as_secs() <= LAST_SECOND);
    let time = if writable { time } else { UNIX_EPOCH };
    humantime::format_rfc3339_seconds(time).to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_time_is_written_in_utc_to_the_second() {
        let time = UNIX_EPOCH + Duration::from_millis(1_700_000_000_999);

        assert_eq!(rfc3339(time), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn the_last_second_of_the_year_9999_is_written() {
        let time = UNIX_EPOCH + Duration::from_secs(LAST_SECOND);

        assert_eq!(rfc3339(time), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn a_time_that_cannot_be_written_is_the_epoch() {
        let before = UNIX_EPOCH
            .checked_sub(Duration::from_secs(1))
            .expect("a time before the epoch");
        let after = UNIX_EPOCH + Duration::from_secs(LAST_SECOND + 1);

        assert_eq!(rfc3339(before), DETERMINISTIC_GENERATED_AT);
        assert_eq!(rfc3339(after), DETERMINISTIC_GENERATED_AT);
    }

    #[test]
    fn a_fixed_clock_gives_the_epoch_and_no_duration() {
        assert_eq!(Clock::Fixed.generated_at(), DETERMINISTIC_GENERATED_AT);
        assert_eq!(Clock::Fixed.duration_ms(Instant::now()), None);
        assert_eq!(Clock::new(true), Clock::Fixed);
    }

    #[test]
    fn the_system_clock_gives_a_date_and_a_duration() {
        let date = Clock::System.generated_at();

        assert_eq!(date.len(), "1970-01-01T00:00:00Z".len(), "{date}");
        assert!(date.ends_with('Z'), "{date}");
        assert!(Clock::System.duration_ms(Instant::now()).is_some());
        assert_eq!(Clock::new(false), Clock::System);
    }
}
