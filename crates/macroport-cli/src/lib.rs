// SPDX-License-Identifier: Apache-2.0
//! The renderers of the `macro-port` reports and the words they use, as a
//! library so that tests reach them without running the binary.

#![forbid(unsafe_code)]

pub mod i18n;
pub mod render;
mod wire;
