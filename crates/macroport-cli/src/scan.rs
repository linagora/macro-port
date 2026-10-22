// SPDX-License-Identifier: Apache-2.0
//! `macro-port scan`: the files under the given paths, analysed in parallel and
//! reported together (SPEC M4).
//!
//! Everything impure is here, since the core is pure: the walk of the
//! directories, the threads, the clock, the output files and the console.

mod walk;

pub use walk::{Candidate, EXTENSIONS, report_path, walk};
