// SPDX-License-Identifier: Apache-2.0
//! Linear time, checked without a limit in seconds. A test that fails above a
//! limit in seconds fails on a loaded machine before a slow algorithm does,
//! and a limit high enough to be safe lets a slow algorithm through. What the
//! machine does not change is how the time grows: an input four times as large
//! takes about four times as long when the work is linear, and about sixteen
//! times as long when it is quadratic. [`assert_linear_time`] times the same
//! work on an input and on one four times as large, and fails when, in each of
//! [`ATTEMPTS`] attempts, the larger takes [`MAX_RATIO`] times as long as the
//! smaller, or more.
//!
//! A test of the core that checks a time goes through this module and not
//! through the clock: the `clippy.toml` of the core keeps the clock out of the
//! core, and out of its tests too.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// How many times as much work the large input of a check makes as the small
/// one.
pub const GROWTH: usize = 4;

/// The ratio of the times from which the work is no longer called linear.
/// Linear work gives about [`GROWTH`], a little more when the larger input
/// outgrows the caches, and quadratic work its square, 16: 8 stands between
/// them, with room on both sides for the noise of a machine.
pub const MAX_RATIO: u32 = 8;

/// The most times that the two runs are made. A rise of the load between them
/// makes the work look worse than it is, and a busy spell lasts a while: an
/// attempt that fails is made again, and the first that shows linear time ends
/// the check. A fall of the load makes the work look better, and this rule
/// gives that more than one chance: quadratic work gives twice [`MAX_RATIO`],
/// so it passes only when the load swings by a factor of 2 between the two runs
/// of an attempt.
pub const ATTEMPTS: usize = 3;

/// What the small run counts for at least. Below it the clock and the scheduler
/// decide the ratio and not the work: a large run under [`MAX_RATIO`] times
/// this passes whatever the small one took, and work that grows faster than
/// linear but ends that soon on the large input is not worth a failure.
pub const FLOOR: Duration = Duration::from_millis(20);

/// Whether two runs of the same work show linear time: the large run, on the
/// input that makes [`GROWTH`] times as much work as the input of the small
/// run, took less than [`MAX_RATIO`] times as long as the small run, which
/// counts for [`FLOOR`] at least.
pub fn is_linear(small: Duration, large: Duration) -> bool {
    large < small.max(FLOOR).saturating_mul(MAX_RATIO)
}

/// Runs `work` on the input that `make` gives for `size / GROWTH`, then on the
/// one it gives for `size`, and does both again until the two times show
/// linear time (see [`is_linear`]), in [`ATTEMPTS`] attempts at most: `work` is
/// called up to twice that many times. Gives what `work` made of the large
/// input in the attempt that passed, for the test to check.
///
/// `make(n)` builds an input that makes `work` do about `n` units of work,
/// whatever a unit is: lines, bytes, tokens. Both inputs are built before the
/// clock starts, for the time of building them is not the one that is being
/// compared. When the work is capped, as the tokens of a line are, `size` is
/// the cap and the large input is the one that reaches it. `what` names the
/// case in the message of a failure.
///
/// # Panics
///
/// When `size` is under [`GROWTH`], and when no attempt shows linear time, with
/// the times of every attempt.
pub fn assert_linear_time<I, R>(
    what: &str,
    size: usize,
    make: impl Fn(usize) -> I,
    work: impl Fn(&I) -> R,
) -> R {
    assert!(
        size >= GROWTH,
        "{what}: a size of {size} is under {GROWTH}, and the small input would make no work"
    );
    let small = make(size / GROWTH);
    let large = make(size);
    let attempt = || {
        let (small_time, _) = timed(|| work(&small));
        let (large_time, made) = timed(|| work(&large));
        (small_time, large_time, made)
    };
    match first_linear(attempt) {
        Ok(made) => made,
        Err(attempts) => panic!("{}", failure(what, &attempts)),
    }
}

/// What `attempt` gives, the time on the small input, the time on the large
/// one and a result, on the first of [`ATTEMPTS`] calls in which the times show
/// linear time; or the times of every call when none does.
fn first_linear<R>(
    mut attempt: impl FnMut() -> (Duration, Duration, R),
) -> Result<R, Vec<(Duration, Duration)>> {
    let mut times = Vec::new();
    for _ in 0..ATTEMPTS {
        let (small, large, made) = attempt();
        if is_linear(small, large) {
            return Ok(made);
        }
        times.push((small, large));
    }
    Err(times)
}

/// What `run` gives, and the time it took. The result is kept from the
/// optimiser, so that work nobody looks at is not skipped.
fn timed<R>(run: impl FnOnce() -> R) -> (Duration, R) {
    let started = Instant::now();
    let made = black_box(run());
    (started.elapsed(), made)
}

/// The message of a check that failed: the times of each of its attempts.
fn failure(what: &str, attempts: &[(Duration, Duration)]) -> String {
    let mut message = format!(
        "{what}: {GROWTH} times the input took {MAX_RATIO} times as long or more in each of \
         {ATTEMPTS} attempts (linear work takes about {GROWTH} times as long, quadratic work \
         {}; a run under {FLOOR:?} counts for {FLOOR:?})",
        GROWTH.saturating_mul(GROWTH)
    );
    for (number, &(small, large)) in attempts.iter().enumerate() {
        let ratio = large.as_secs_f64() / small.max(Duration::from_nanos(1)).as_secs_f64();
        message.push_str(&format!(
            "\n  attempt {}: {small:?} then {large:?}, {ratio:.1} times as long",
            number.saturating_add(1)
        ));
    }
    message
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn four_times_as_long_is_linear_and_eight_times_is_not() {
        assert!(is_linear(ms(100), ms(400)));
        assert!(is_linear(ms(100), ms(799)));
        assert!(!is_linear(ms(100), ms(800)));
        assert!(!is_linear(ms(100), ms(1600)));
        // A large input that goes quicker than the small one is no problem.
        assert!(is_linear(ms(100), ms(30)));
    }

    #[test]
    fn a_run_shorter_than_the_floor_counts_for_the_floor() {
        let most = FLOOR.saturating_mul(MAX_RATIO);
        // The small run took no time, or one millisecond: the floor counts.
        for small in [Duration::ZERO, ms(1), FLOOR] {
            assert!(is_linear(small, most - ms(1)), "{small:?}");
            assert!(!is_linear(small, most), "{small:?}");
        }
        // Above the floor, the small run counts for what it took.
        let small = FLOOR + ms(10);
        assert!(is_linear(small, small.saturating_mul(MAX_RATIO) - ms(1)));
        assert!(!is_linear(small, small.saturating_mul(MAX_RATIO)));
    }

    #[test]
    fn the_first_attempt_that_shows_linear_time_ends_the_check() {
        let script = [
            (ms(100), ms(2000), "slow"),
            (ms(100), ms(400), "good"),
            (ms(1), ms(1), "never"),
        ];
        let calls = Cell::new(0);
        let found = first_linear(|| {
            let step = script[calls.get()];
            calls.set(calls.get() + 1);
            step
        });
        assert_eq!(found, Ok("good"));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn the_times_of_every_attempt_come_back_when_none_shows_linear_time() {
        let calls = Cell::new(0);
        let found = first_linear(|| {
            calls.set(calls.get() + 1);
            (ms(100), ms(1600), ())
        });
        assert_eq!(found, Err(vec![(ms(100), ms(1600)); ATTEMPTS]));
        assert_eq!(calls.get(), ATTEMPTS);
    }

    #[test]
    fn the_message_of_a_failure_gives_the_times_of_each_attempt() {
        let message = failure("a case", &[(ms(100), ms(1600)), (ms(50), ms(800))]);
        assert!(message.starts_with("a case: 4 times the input took 8 times"));
        assert!(message.contains("attempt 1: 100ms then 1.6s, 16.0 times as long"));
        assert!(message.contains("attempt 2: 50ms then 800ms, 16.0 times as long"));
        // Nothing to divide by.
        assert!(failure("a case", &[(Duration::ZERO, ms(1))]).contains("attempt 1: 0ns then 1ms"));
    }
}
