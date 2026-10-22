// SPDX-License-Identifier: Apache-2.0
//! The check of linear time, on work that sleeps: it takes the time that it is
//! told to, so the speed of the machine does not matter. A busy machine can
//! only wake a sleeper late, and each test says how late it can be before the
//! verdict changes.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::sleep;
use std::time::Duration;

use macroport_testkit::linear::{ATTEMPTS, assert_linear_time};

/// How many times an attempt calls the work: on the quarter, on the whole, on
/// the quarter again.
const RUNS: usize = 3;

/// The number as a count of milliseconds.
fn ms(n: usize) -> Duration {
    Duration::from_millis(u64::try_from(n).expect("a small number"))
}

/// The failure of the check on quadratic work that sleeps, 50 ms on the quarter
/// of 16 and 800 ms on the whole, with `stall` added to the first run of each
/// attempt, which is on the quarter. Gives the message of the failure and the
/// number of calls of the work.
fn failure_on_quadratic_sleeps(stall: Duration) -> (String, usize) {
    let calls = Cell::new(0);
    let quadratic = |&n: &usize| {
        calls.set(calls.get() + 1);
        let late = if calls.get() % RUNS == 1 {
            stall
        } else {
            Duration::ZERO
        };
        let sleeps = 3125 * u64::try_from(n * n).expect("a small number");
        sleep(Duration::from_micros(sleeps) + late);
    };
    let caught = catch_unwind(AssertUnwindSafe(|| {
        assert_linear_time("quadratic sleeps", 16, |n| n, quadratic)
    }));
    let payload = caught.expect_err("the check let quadratic work through");
    let message = payload.downcast::<String>().expect("a formatted message");
    (*message, calls.get())
}

#[test]
fn work_that_takes_four_times_as_long_on_four_times_the_input_passes() {
    // 50 ms for the quarter, 200 ms for the whole. The verdict changes only if
    // the whole is measured at 400 ms or more, 200 ms late, in all 3 attempts.
    let made = assert_linear_time(
        "linear sleeps",
        200,
        |n| n,
        |&n| {
            sleep(ms(n));
            n
        },
    );
    // What the work made of the large input.
    assert_eq!(made, 200);
}

#[test]
fn work_that_takes_sixteen_times_as_long_on_four_times_the_input_fails_every_attempt() {
    // 50 ms for the quarter, 800 ms for the whole: the square of the growth.
    // An attempt in which both runs on the quarter are measured at over 100 ms,
    // 50 ms late, passes the work and ends the check, so the test fails.
    let (message, calls) = failure_on_quadratic_sleeps(Duration::ZERO);
    assert!(
        message.starts_with("quadratic sleeps: 4 times the input took 8 times as long or more"),
        "{message}"
    );
    // One line for each attempt.
    assert_eq!(
        message.matches("\n  attempt ").count(),
        ATTEMPTS,
        "{message}"
    );
    assert_eq!(calls, RUNS * ATTEMPTS);
}

#[test]
fn a_stall_on_one_run_of_the_small_input_does_not_hide_quadratic_work() {
    // The first run on the quarter sleeps 150 ms more in each attempt: its 200
    // ms would pass the work against the 800 of the whole if it were the only
    // one to count. The second run on the quarter is not stalled and its time
    // is the one that counts. The verdict changes if that run is measured at
    // over 100 ms, 50 ms late, in one attempt.
    let (message, calls) = failure_on_quadratic_sleeps(ms(150));
    assert_eq!(
        message.matches("\n  attempt ").count(),
        ATTEMPTS,
        "{message}"
    );
    assert_eq!(calls, RUNS * ATTEMPTS);
}

#[test]
fn an_attempt_that_meets_a_busy_machine_is_made_again() {
    // The second call is the run on the large input in the first attempt: it
    // takes 800 ms for the 200 that it needs, against the 50 of the small one.
    // The next attempt is quiet. Both runs on the small input measured at over
    // 100 ms, 50 ms late, would pass the first attempt, and the test would fail
    // on the count of calls.
    let calls = Cell::new(0);
    let work = |&n: &usize| {
        calls.set(calls.get() + 1);
        sleep(ms(if calls.get() == 2 { 800 } else { n }));
        n
    };
    let made = assert_linear_time("busy once", 200, |n| n, work);
    assert_eq!(made, 200);
    assert_eq!(calls.get(), 2 * RUNS);
}

#[test]
#[should_panic(expected = "a size of 3 is under 4")]
fn a_size_under_the_growth_is_refused() {
    assert_linear_time("too small", 3, |n| n, |&n| n);
}
