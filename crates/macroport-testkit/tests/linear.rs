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
    // 50 ms for the quarter, 800 ms for the whole: the square of the growth.
    // One attempt that measures the quarter at over 100 ms, 50 ms late, passes
    // the work and ends the check, so the test fails.
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
    // One line for each attempt, and two runs in each.
    assert_eq!(
        message.matches("\n  attempt ").count(),
        ATTEMPTS,
        "{message}"
    );
    assert_eq!(calls.get(), 2 * ATTEMPTS);
}

#[test]
fn an_attempt_that_meets_a_busy_machine_is_made_again() {
    // The second call is the first run on the large input: it takes 800 ms for
    // the 200 that it needs, against the 50 of the small one. The next attempt
    // is quiet. A small run measured at over 100 ms, 50 ms late, would pass the
    // first attempt, and the test would fail on the count of calls.
    let calls = Cell::new(0);
    let work = |&n: &usize| {
        calls.set(calls.get() + 1);
        sleep(ms(if calls.get() == 2 { 800 } else { n }));
        n
    };
    let made = assert_linear_time("busy once", 200, |n| n, work);
    assert_eq!(made, 200);
    assert_eq!(calls.get(), 4);
}

#[test]
#[should_panic(expected = "a size of 3 is under 4")]
fn a_size_under_the_growth_is_refused() {
    assert_linear_time("too small", 3, |n| n, |&n| n);
}
