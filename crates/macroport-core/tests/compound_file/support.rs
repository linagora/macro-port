// SPDX-License-Identifier: Apache-2.0
//! Containers to read, and the bytes to patch in them.

use std::io::{Cursor, Write};

use cfb::{CompoundFile, Version};
use macroport_core::container::{CfbFile, ContainerError};
use macroport_core::report::Status;

pub const END_OF_CHAIN: u32 = 0xFFFF_FFFE;

// Where the fields sit in the header of a container (MS-CFB 2.2) and in a
// directory entry (2.6.1), which is 128 bytes long.
pub const MAJOR_VERSION: usize = 0x1A;
pub const FIRST_DIRECTORY_SECTOR: usize = 0x30;
pub const FIRST_MINIFAT_SECTOR: usize = 0x3C;
const HEADER_DIFAT: usize = 0x4C;
const ENTRY_LEN: usize = 128;
const NAME_LENGTH: usize = 64;
pub const CHILD: usize = 76;
pub const START_SECTOR: usize = 116;
pub const STREAM_SIZE: usize = 120;

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

/// A container holding `count` streams of these bytes in its root.
pub fn container_of_streams(version: Version, count: usize, data: &[u8]) -> Vec<u8> {
    let names: Vec<String> = (0..count).map(|index| format!("s{index}")).collect();
    let streams: Vec<(&str, &[u8])> = names.iter().map(|name| (name.as_str(), data)).collect();
    container(version, &streams)
}

/// The path of a storage `depth` levels down, in a chain of storages.
pub fn nested_path(depth: usize) -> String {
    vec!["d"; depth].join("/")
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

pub fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

pub fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn put_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

// The tables of a container written by the `cfb` crate, which is small
// enough for its FAT to be reached through the header's DIFAT alone.

fn sector_len(bytes: &[u8]) -> usize {
    if u16_at(bytes, MAJOR_VERSION) == 4 {
        4096
    } else {
        512
    }
}

/// Where `sector` starts in the file.
pub fn sector_at(bytes: &[u8], sector: u32) -> usize {
    (sector as usize + 1) * sector_len(bytes)
}

fn fat_entry_at(bytes: &[u8], sector: u32) -> usize {
    let per_sector = sector_len(bytes) / 4;
    let fat_sector = u32_at(bytes, HEADER_DIFAT + 4 * (sector as usize / per_sector));
    sector_at(bytes, fat_sector) + 4 * (sector as usize % per_sector)
}

pub fn fat_next(bytes: &[u8], sector: u32) -> u32 {
    u32_at(bytes, fat_entry_at(bytes, sector))
}

pub fn put_fat_next(bytes: &mut [u8], sector: u32, next: u32) {
    let at = fat_entry_at(bytes, sector);
    put_u32(bytes, at, next);
}

/// Where directory entry number `index` sits, following the directory chain.
pub fn directory_entry_at(bytes: &[u8], index: usize) -> usize {
    let per_sector = sector_len(bytes) / ENTRY_LEN;
    let mut sector = u32_at(bytes, FIRST_DIRECTORY_SECTOR);
    for _ in 0..index / per_sector {
        sector = fat_next(bytes, sector);
    }
    sector_at(bytes, sector) + ENTRY_LEN * (index % per_sector)
}

/// Where the directory entry named `name` sits, among the first 64: the
/// containers of these tests hold fewer entries.
pub fn offset_of_entry(bytes: &[u8], name: &str) -> usize {
    (0..64)
        .map(|index| directory_entry_at(bytes, index))
        .find(|&at| {
            // The length counts the two bytes of the terminating null.
            let length = usize::from(u16_at(bytes, at + NAME_LENGTH)).saturating_sub(2);
            let units: Vec<u16> = bytes[at..at + length]
                .chunks(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units) == name
        })
        .unwrap_or_else(|| panic!("no directory entry named {name}"))
}

/// The sectors of the chain that starts at the stream `name`, which must be
/// 4,096 bytes or more: a shorter one lives in the mini stream.
pub fn chain_of(bytes: &[u8], name: &str) -> Vec<u32> {
    let mut sector = u32_at(bytes, offset_of_entry(bytes, name) + START_SECTOR);
    let mut chain = Vec::new();
    while sector != END_OF_CHAIN {
        chain.push(sector);
        sector = fat_next(bytes, sector);
    }
    chain
}
