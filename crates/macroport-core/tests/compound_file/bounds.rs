// SPDX-License-Identifier: Apache-2.0
//! Tables that ask for more than a file of that length can hold. The scan of
//! the tables (ADR-0003) refuses them before the `cfb` crate allocates for
//! them; its messages tell its refusals from the crate's.

use cfb::Version;
use macroport_core::container::{CfbFile, MAX_DIRECTORY_ENTRIES, read_ooxml};
use macroport_core::report::Status;
use macroport_testkit::corpus;

use crate::support::{
    DIFAT_SECTOR_COUNT, END_OF_CHAIN, FAT_SECTOR_COUNT, FIRST_DIFAT_SECTOR, FIRST_DIRECTORY_SECTOR,
    FIRST_MINIFAT_SECTOR, FREE_SECTOR, HEADER_DIFAT, assert_error, chain_from, chain_of, container,
    container_of_streams, open, pattern, put_fat_next, put_u32, refused, sector_at, sector_len,
    u32_at,
};

// The scan shares its codes with the crate's own refusals (`cfb.fat`), so the
// tests tell the two apart by the words of the messages.
const DIRECTORY_TOO_LONG: &str = "the directory takes more than";
const DIFAT_TOO_LARGE: &str = "the DIFAT is larger than";
const MINIFAT_TOO_LONG: &str = "the MiniFAT takes more than";

/// FAT sectors that the header lists, at most.
const HEADER_DIFAT_ENTRIES: usize = 109;

/// FAT sectors that a DIFAT sector of a version 3 container lists: 128 entries
/// less the one that chains to the next DIFAT sector.
const DIFAT_ENTRIES_PER_SECTOR: usize = 127;

/// Whether the scan refuses these bytes, for the reason that `phrase` names.
fn scan_refuses(bytes: &[u8], phrase: &str) -> bool {
    CfbFile::open(bytes).is_err_and(|error| error.message.contains(phrase))
}

/// Makes the header list `count` FAT sectors, all of them `entry`.
fn list_in_header(bytes: &mut [u8], count: usize, entry: u32) {
    for index in 0..HEADER_DIFAT_ENTRIES {
        let value = if index < count { entry } else { FREE_SECTOR };
        put_u32(bytes, HEADER_DIFAT + 4 * index, value);
    }
    put_u32(bytes, FAT_SECTOR_COUNT, count as u32);
}

/// Appends `count` DIFAT sectors, every entry of which is `entry`, and chains
/// them from the header.
fn chain_difat_sectors(bytes: &mut Vec<u8>, count: usize, entry: u32) {
    let len = sector_len(bytes);
    assert_eq!(bytes.len() % len, 0, "the container ends on a sector");
    let first = bytes.len() / len - 1;
    for index in 0..count {
        let next = if index + 1 == count {
            END_OF_CHAIN
        } else {
            (first + index + 1) as u32
        };
        for _ in 0..len / 4 - 1 {
            bytes.extend_from_slice(&entry.to_le_bytes());
        }
        bytes.extend_from_slice(&next.to_le_bytes());
    }
    put_u32(bytes, FIRST_DIFAT_SECTOR, first as u32);
    put_u32(bytes, DIFAT_SECTOR_COUNT, count as u32);
}

#[test]
fn a_directory_too_long_for_2000_entries_is_refused_before_it_is_read() {
    for version in [Version::V3, Version::V4] {
        let mut bytes = container_of_streams(version, 2_400, b"");
        // Garbage in the last sector of the directory: the crate would name
        // it with another code, had it read it.
        let first = u32_at(&bytes, FIRST_DIRECTORY_SECTOR);
        let last = *chain_from(&bytes, first).last().unwrap();
        let (at, len) = (sector_at(&bytes, last), sector_len(&bytes));
        bytes[at..at + len].fill(0xAA);

        let error = refused(&bytes);
        assert_error(&error, Status::Corrupted, "cfb.too_many_entries");
        assert!(error.message.contains(DIRECTORY_TOO_LONG), "{error}");
    }
}

#[test]
fn the_scan_allows_the_directory_the_sectors_that_2000_entries_fill() {
    // 501 sectors of 512 bytes are too many for 2,000 entries: the scan
    // refuses the 2,001 of this file.
    let bytes = container_of_streams(Version::V3, MAX_DIRECTORY_ENTRIES, b"");
    assert!(scan_refuses(&bytes, DIRECTORY_TOO_LONG));

    // 63 sectors of 4,096 bytes are within the bound, and hold 2,016 entries:
    // the same 2,001 entries pass the scan, and the walk of the tree refuses
    // them.
    let bytes = container_of_streams(Version::V4, MAX_DIRECTORY_ENTRIES, b"");
    assert_error(&refused(&bytes), Status::Corrupted, "cfb.too_many_entries");
    assert!(!scan_refuses(&bytes, DIRECTORY_TOO_LONG));
}

#[test]
fn a_difat_chain_far_longer_than_the_file_can_need_is_refused() {
    // Each entry of the DIFAT names a sector of the file, so the crate would
    // read a whole sector of FAT for it: 128 times the entry in a version 3
    // file, 1,024 times in a version 4 file. A free entry stops the crate at
    // once, so the bomb lists a sector in all of them.
    for (version, count) in [(Version::V3, 200), (Version::V4, 20)] {
        let mut bytes = container(version, &[("a", b"a")]);
        chain_difat_sectors(&mut bytes, count, 1);
        list_in_header(&mut bytes, HEADER_DIFAT_ENTRIES, 1);

        let error = refused(&bytes);
        assert_error(&error, Status::Corrupted, "cfb.fat");
        assert!(error.message.contains(DIFAT_TOO_LARGE), "{error}");
    }
}

#[test]
fn a_small_file_may_list_16_fat_sectors_and_chain_16_difat_sectors_and_not_17() {
    let small = container(Version::V3, &[("a", b"a")]);
    for (count, is_refused) in [(16, false), (17, true)] {
        let mut listed = small.clone();
        list_in_header(&mut listed, count, 1);
        // DIFAT sectors of padding list no FAT sector, but the chain is bounded.
        let mut chained = small.clone();
        chain_difat_sectors(&mut chained, count, FREE_SECTOR);
        for (kind, bytes) in [("listed", listed), ("chained", chained)] {
            let crossed = scan_refuses(&bytes, DIFAT_TOO_LARGE);
            assert_eq!(crossed, is_refused, "{kind} {count}");
        }
    }
}

#[test]
fn a_difat_chain_that_loops_is_a_loop() {
    let mut bytes = container(Version::V3, &[("a", b"a")]);
    chain_difat_sectors(&mut bytes, 1, FREE_SECTOR);
    // The DIFAT sector names itself as the next one.
    let first = u32_at(&bytes, FIRST_DIFAT_SECTOR);
    let next_at = sector_at(&bytes, first) + sector_len(&bytes) - 4;
    put_u32(&mut bytes, next_at, first);

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.fat_loop");
}

#[test]
fn a_minifat_longer_than_the_file_can_need_is_refused() {
    let bytes = container(Version::V3, &[("big", &pattern(20_000)), ("small", b"hi")]);
    let sectors = bytes.len() / 512 - 1;
    // A MiniFAT sector lists 128 mini sectors of 64 bytes, which fill 16
    // sectors of the file. The scan allows two more than that.
    let limit = sectors / 16 + 2;
    let chain = chain_of(&bytes, "big");
    assert!(limit < chain.len(), "{limit} sectors, {chain:?}");

    let minifat = u32_at(&bytes, FIRST_MINIFAT_SECTOR);
    let directory = chain_from(&bytes, u32_at(&bytes, FIRST_DIRECTORY_SECTOR));
    let directory_end = *directory.last().unwrap();
    for (length, is_refused) in [(limit, false), (limit + 1, true)] {
        // The crate ends a chain at a pointer beyond the file (0x1000_0000)
        // instead of refusing it, and reads the MiniFAT all the same: the scan
        // must too.
        for directory_next in [END_OF_CHAIN, 0x1000_0000] {
            let mut bytes = bytes.clone();
            put_fat_next(&mut bytes, directory_end, directory_next);
            // The MiniFAT goes on into the end of the chain of "big".
            put_fat_next(&mut bytes, minifat, chain[chain.len() - (length - 1)]);
            assert_eq!(
                scan_refuses(&bytes, MINIFAT_TOO_LONG),
                is_refused,
                "{length} {directory_next:#x}"
            );
        }
    }
}

#[test]
fn a_file_whose_fat_needs_difat_sectors_is_not_refused() {
    // 8 MB need more FAT sectors than the header lists, so that the DIFAT
    // takes a sector.
    let big = pattern(8 * 1024 * 1024);
    let bytes = container(Version::V3, &[("big", &big)]);
    assert_eq!(u32_at(&bytes, DIFAT_SECTOR_COUNT), 1);
    assert!(open(&bytes).read_stream("big").unwrap().as_ref() == Some(&big));

    // Some writers pad the last DIFAT sector with zeros, not free entries.
    // The crate drops them, and so does the scan.
    let mut padded = bytes;
    let at = sector_at(&padded, u32_at(&padded, FIRST_DIFAT_SECTOR));
    for entry in (0..DIFAT_ENTRIES_PER_SECTOR).map(|index| at + 4 * index) {
        if u32_at(&padded, entry) == FREE_SECTOR {
            put_u32(&mut padded, entry, 0);
        }
    }
    assert!(open(&padded).read_stream("big").unwrap().as_ref() == Some(&big));
}

#[test]
fn the_vba_projects_of_the_fixtures_are_not_refused() {
    let mut opened = 0;
    for name in corpus::files("fixtures/generated", &["xlsm", "xlsb"]) {
        let Ok(package) = read_ooxml(&corpus::read(&name)) else {
            continue;
        };
        if let Some(project) = package.vba_project {
            CfbFile::open(&project).unwrap_or_else(|error| panic!("{name}: {error}"));
            opened += 1;
        }
    }
    assert!(opened > 100, "{opened} projects");
}
