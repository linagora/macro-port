// SPDX-License-Identifier: Apache-2.0
//! Renderers: a report in, the text of an output file out.
//!
//! Every string of a report that comes from an analysed file (paths, module
//! and procedure names, snippets, references, error messages) is untrusted.
//! Each renderer protects it for its own format (SPEC section 8).

pub mod csv;
