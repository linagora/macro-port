// SPDX-License-Identifier: Apache-2.0
//! The commands of `macro-port`, their renderers and the words they use, as a
//! library so that tests reach them without running the binary.

#![forbid(unsafe_code)]

pub mod args;
pub mod clock;
pub mod command;
pub mod console;
pub mod i18n;
pub mod render;
pub mod reports;
pub mod rules_check;
pub mod scan;
mod wire;
