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
        if self == Self::Fixed {
            return DETERMINISTIC_GENERATED_AT.to_owned();
        }
        // humantime writes the years 1970 to 9999 and panics outside them: a
        // clock that is set wrong gives the epoch, not a crash.
        let now = SystemTime::now();
        let writable = now
            .duration_since(UNIX_EPOCH)
            .is_ok_and(|elapsed| elapsed.as_secs() <= LAST_SECOND);
        let time = if writable { now } else { UNIX_EPOCH };
        humantime::format_rfc3339_seconds(time).to_string()
    }

    /// The milliseconds since `started`, none for a fixed clock.
    pub fn duration_ms(self, started: Instant) -> Option<u64> {
        (self == Self::System)
            .then(|| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
    }
}
