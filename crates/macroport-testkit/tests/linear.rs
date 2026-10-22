// SPDX-License-Identifier: Apache-2.0
//! The check of linear time on the clock of the machine, with work that sleeps:
//! it takes the time that it is told to, so the speed of the machine does not
//! matter. A busy machine can only wake a sleeper late, and each test says how
//! late it can be before the verdict changes. What the check decides, which
//! attempt ends it and what the small input counts for, is tested in the module
//! itself, on a clock that a script moves.

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
    // 50 ms for the quarter of 16, 800 ms for the whole: the square of the
    // growth. An attempt in which the two runs on the quarter are late by more
    // than 100 ms between them passes the work and ends the check, so the test
    // fails.
    let calls = Cell::new(0);
    let quadratic = |&n: &usize| {
        calls.set(calls.get() + 1);
        sleep(Duration::from_micros(
            3125 * u64::try_from(n * n).expect("a small number"),
        ));
    };
    let caught = catch_unwind(AssertUnwindSafe(|| {
        assert_linear_time("quadratic sleeps", 16, |n| n, quadratic)
    }));
    let payload = caught.expect_err("the check let quadratic work through");
    let message = payload.downcast::<String>().expect("a formatted message");
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
    assert_eq!(calls.get(), RUNS * ATTEMPTS);
}

#[test]
#[should_panic(expected = "a size of 3 is under 4")]
fn a_size_under_the_growth_is_refused() {
    assert_linear_time("too small", 3, |n| n, |&n| n);
}
