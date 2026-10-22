// SPDX-License-Identifier: Apache-2.0
//! The commands of `macro-port`, their renderers and the words they use, as a
//! library so that tests reach them without running the binary.

#![forbid(unsafe_code)]

pub mod clock;
pub mod i18n;
pub mod render;
pub mod scan;
mod wire;
