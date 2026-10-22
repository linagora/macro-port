// SPDX-License-Identifier: Apache-2.0
//! Linear time, checked without a limit in seconds. A test that fails above a
//! limit in seconds fails on a loaded machine before a slow algorithm does,
//! and a limit high enough to be safe lets a slow algorithm through. What the
//! machine does not change is how the time grows: an input four times as large
//! takes about four times as long when the work is linear, and about sixteen
//! times as long when it is quadratic. [`assert_linear_time`] times the same
//! work on an input and on one four times as large, and fails when, in each of
//! [`ATTEMPTS`] attempts, the larger takes [`MAX_RATIO`] times as long as the
//! smaller, or more. The smaller is run before and after the larger, and the
//! mean of its two times is the one that counts.
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
/// them, with room on both sides for the noise of a machine. It is also the
/// ratio of work that grows as the power 1.5 of its input: work that grows more
/// slowly than that, n log n included, passes, and the check tells quadratic
/// work from linear work, nothing finer.
pub const MAX_RATIO: u32 = 8;

/// The most times that the runs are made. Noise only ever lengthens a run. On
/// the large run it makes the work look worse than it is, and a busy spell lasts
/// a while: an attempt that fails is made again, and the first that shows linear
/// time ends the check. On the small run it makes the work look better, and
/// quadratic work then passes by luck, which the several attempts make likelier:
/// the small input is run before and after the large one for that, and the mean
/// of the two times counts, so that a stall on one of the runs weighs half. The
/// mean and not the lower time, because a load that grows or fades during an
/// attempt then weighs the same on both sides of the large run: the lower time is
/// that of the quieter side, and makes the work look worse than it is whenever
/// the machine is busy.
pub const ATTEMPTS: usize = 3;

/// What the small run counts for at least. A busy machine delays a run by tens
/// of milliseconds whatever the work is, and below this the scheduler decides
/// the ratio and not the work: a large run under [`MAX_RATIO`] times this passes
/// whatever the small one took, and work that grows faster than linear but ends
/// that soon on the large input is not worth a failure.
pub const FLOOR: Duration = Duration::from_millis(50);

/// Whether two runs of the same work show linear time: the large run, on the
/// input that makes [`GROWTH`] times as much work as the input of the small
/// run, took less than [`MAX_RATIO`] times as long as the small run, which
/// counts for [`FLOOR`] at least.
pub fn is_linear(small: Duration, large: Duration) -> bool {
    large < small.max(FLOOR).saturating_mul(MAX_RATIO)
}

/// Runs `work` on the input that `make` gives for `size / GROWTH`, on the one it
/// gives for `size`, and on the small one again. The attempt shows linear time
/// when the large run is under [`MAX_RATIO`] times the mean of the two small
/// runs (see [`is_linear`]). Makes [`ATTEMPTS`] attempts at most and stops at the
/// first that shows linear time, so `work` is called up to three times that many
/// times. Gives what `work` made of the large input in that attempt, for the
/// test to check.
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
    let origin = Instant::now();
    check(what, size, make, work, || origin.elapsed())
}

/// [`assert_linear_time`] on a clock that is given, which tells the time that
/// has gone by since a moment of its own choice. The tests of this module
/// script it: nothing in them waits for the machine.
fn check<I, R>(
    what: &str,
    size: usize,
    make: impl Fn(usize) -> I,
    work: impl Fn(&I) -> R,
    clock: impl Fn() -> Duration,
) -> R {
    assert!(
        size >= GROWTH,
        "{what}: a size of {size} is under {GROWTH}, and the small input would make no work"
    );
    let small = make(size / GROWTH);
    let large = make(size);
    // The result is kept from the optimiser, so that work nobody looks at is not
    // skipped.
    let timed = |input: &I| {
        let started = clock();
        let made = black_box(work(input));
        (clock().saturating_sub(started), made)
    };
    let attempt = || {
        let (before, _) = timed(&small);
        let (large_time, made) = timed(&large);
        let (after, _) = timed(&small);
        (small_time(before, after), large_time, made)
    };
    match first_linear(attempt) {
        Ok(made) => made,
        Err(attempts) => panic!("{}", failure(what, &attempts)),
    }
}

/// What the small input counts for in an attempt, from its two runs: their mean.
fn small_time(before: Duration, after: Duration) -> Duration {
    before.saturating_add(after) / 2
}

/// What `attempt` gives, the time on the small input, the time on the large
/// one and a result, on the first of [`ATTEMPTS`] calls in which the times show
/// linear time; or the times of every call when none does. The time on the
/// small input is the mean of its two runs.
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

/// The message of a check that failed: the times of each of its attempts.
fn failure(what: &str, attempts: &[(Duration, Duration)]) -> String {
    let mut message = format!(
        "{what}: {GROWTH} times the input took {MAX_RATIO} times as long or more in each of \
         {ATTEMPTS} attempts (linear work takes about {GROWTH} times as long, quadratic work \
         {}; the small input is run before and after the large one and the mean of its two \
         times counts; a time under {FLOOR:?} counts for {FLOOR:?})",
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
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A clock that only the work moves: the n-th call of the work lasts the n-th
    /// time of the script, in milliseconds, and takes no time of the machine.
    struct Scripted<'a> {
        script: &'a [u64],
        calls: Cell<usize>,
        now: Cell<Duration>,
    }

    impl<'a> Scripted<'a> {
        fn new(script: &'a [u64]) -> Self {
            Scripted {
                script,
                calls: Cell::new(0),
                now: Cell::new(Duration::ZERO),
            }
        }

        /// The work: moves the clock by its time in the script and gives its
        /// input back.
        fn work(&self, input: &usize) -> usize {
            let call = self.calls.get();
            let lasts = self.script.get(call).expect("a time in the script");
            self.calls.set(call + 1);
            self.now.set(self.now.get() + ms(*lasts));
            *input
        }

        fn clock(&self) -> Duration {
            self.now.get()
        }

        fn calls(&self) -> usize {
            self.calls.get()
        }
    }

    /// The check on a script of times, for a size of 200: what the work made of
    /// the large input, or the message of the failure; and how many times the work
    /// was called.
    fn run(script: &[u64]) -> (Result<usize, String>, usize) {
        let scripted = Scripted::new(script);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            check(
                "scripted",
                200,
                |n| n,
                |n| scripted.work(n),
                || scripted.clock(),
            )
        }));
        let outcome =
            outcome.map_err(|payload| *payload.downcast::<String>().expect("a formatted message"));
        (outcome, scripted.calls())
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
    fn the_small_input_counts_for_the_mean_of_its_two_runs() {
        assert_eq!(small_time(ms(50), ms(150)), ms(100));
        assert_eq!(small_time(ms(150), ms(50)), ms(100));
        assert_eq!(
            small_time(Duration::ZERO, ms(1)),
            Duration::from_micros(500)
        );
        assert_eq!(small_time(Duration::MAX, Duration::MAX), Duration::MAX / 2);
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

    #[test]
    fn work_that_takes_four_times_as_long_on_four_times_the_input_passes_at_once() {
        // 50 ms on the quarter, 200 on the whole, 50 on the quarter again.
        let (outcome, calls) = run(&[50, 200, 50]);
        // What the work made of the large input, in the one attempt.
        assert_eq!(outcome, Ok(200));
        assert_eq!(calls, 3);
    }

    #[test]
    fn work_that_takes_sixteen_times_as_long_on_four_times_the_input_fails_every_attempt() {
        // 50 ms on the quarter, 800 on the whole: the square of the growth.
        let (outcome, calls) = run(&[50, 800, 50].repeat(ATTEMPTS));
        let message = outcome.expect_err("quadratic work passed");
        assert_eq!(calls, 3 * ATTEMPTS);
        assert!(
            message.starts_with("scripted: 4 times the input took 8 times as long or more"),
            "{message}"
        );
        assert_eq!(
            message.matches("\n  attempt ").count(),
            ATTEMPTS,
            "{message}"
        );
        assert!(
            message.contains("attempt 3: 50ms then 800ms, 16.0 times as long"),
            "{message}"
        );
    }

    #[test]
    fn an_attempt_that_meets_a_busy_spell_is_made_again() {
        // The large run of the first attempt takes 800 ms for the 200 that it
        // needs, against 50 on the quarter. The second attempt is quiet.
        let (outcome, calls) = run(&[50, 800, 50, 50, 200, 50]);
        assert_eq!(outcome, Ok(200));
        assert_eq!(calls, 6);
    }

    #[test]
    fn a_machine_that_is_busier_on_one_side_of_the_large_run_is_not_blamed() {
        // The machine gets busy after the first run on the quarter, or quiet
        // after it, and the whole runs in the middle of the change: 600 ms is 12
        // times the lower of the two runs on the quarter, 50, and 6 times their
        // mean, 100. The work passes the first attempt, whichever way it goes.
        for script in [[50, 600, 150], [150, 600, 50]] {
            let (outcome, calls) = run(&script);
            assert_eq!(outcome, Ok(200), "{script:?}");
            assert_eq!(calls, 3, "{script:?}");
        }
    }

    #[test]
    fn a_stall_on_one_run_of_the_small_input_does_not_hide_quadratic_work() {
        // 50 ms on the quarter and 800 on the whole, as above, and the first run
        // on the quarter of each attempt lasts 150 ms. Taken alone it would pass
        // the work, 800 against 8 times 150, and it counts for half: the mean is
        // 100, and the 800 of the whole is not under 8 times that. A stall of one
        // millisecond more would let the work through.
        let (outcome, calls) = run(&[150, 800, 50].repeat(ATTEMPTS));
        let message = outcome.expect_err("quadratic work passed");
        assert_eq!(calls, 3 * ATTEMPTS);
        assert_eq!(
            message.matches("\n  attempt ").count(),
            ATTEMPTS,
            "{message}"
        );
    }
}
