// SPDX-License-Identifier: Apache-2.0
//! Containers to read, and the bytes to patch in them.

use std::io::{Cursor, Write};

use cfb::{CompoundFile, Version};
use macroport_core::container::{CfbFile, ContainerError};
use macroport_core::report::Status;

/// A container holding these streams, and the storages of their paths.
///
/// The one place that writes a compound file, which `clippy.toml` bans from
/// the core: tests build their containers here.
#[allow(clippy::disallowed_methods)]
pub fn container(version: Version, streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut file = CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
    for (path, data) in streams {
        if let Some((storage, _)) = path.rsplit_once('/') {
            file.create_storage_all(format!("/{storage}")).unwrap();
        }
        let mut stream = file.create_stream(format!("/{path}")).unwrap();
        stream.write_all(data).unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

/// The `length` bytes that tell where they come from. Their period is a prime,
/// which divides no sector size: neighbouring sectors hold different bytes.
pub fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}

pub fn open(bytes: &[u8]) -> CfbFile<'_> {
    CfbFile::open(bytes).unwrap_or_else(|error| panic!("the container should open: {error}"))
}

pub fn refused(bytes: &[u8]) -> ContainerError {
    CfbFile::open(bytes).expect_err("the container should be refused")
}

pub fn assert_error(error: &ContainerError, status: Status, code: &str) {
    assert_eq!((error.status, error.code), (status, code), "{error}");
}

/// What every refusal of the CFB layer looks like.
pub fn assert_typed(error: &ContainerError) {
    assert!(error.code.starts_with("cfb."), "{error}");
    assert_eq!(error.status, Status::Corrupted, "{error}");
    assert!(!error.message.is_empty(), "{error}");
}

pub fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
