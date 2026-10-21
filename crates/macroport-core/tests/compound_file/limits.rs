// SPDX-License-Identifier: Apache-2.0
//! The limits of SPEC M1 (2,000 directory entries, storages 32 deep, streams
//! of 64 MB) and the chains that loop: each is crossed, and refused with its
//! code.

use cfb::Version;
use macroport_core::container::{MAX_DIRECTORY_ENTRIES, MAX_STORAGE_DEPTH, MAX_STREAM_SIZE};
use macroport_core::report::Status;

use crate::support::{
    CHILD, END_OF_CHAIN, FIRST_DIRECTORY_SECTOR, FIRST_MINIFAT_SECTOR, START_SECTOR, STREAM_SIZE,
    assert_error, chain_of, container, container_of_streams, directory_entry_at, fat_next,
    nested_path, offset_of_entry, open, pattern, put_fat_next, put_u16, put_u32, put_u64, refused,
    sector_at, u32_at,
};

#[test]
fn the_directory_may_hold_2000_entries_and_not_one_more() {
    // The root is an entry: it leaves room for one stream fewer.
    for version in [Version::V3, Version::V4] {
        let at_the_limit = container_of_streams(version, MAX_DIRECTORY_ENTRIES - 1, b"");
        let mut file = open(&at_the_limit);
        assert!(file.has_stream("s0"), "{version:?}");
        assert_eq!(file.read_stream("s1998").unwrap(), Some(Vec::new()));

        let over = container_of_streams(version, MAX_DIRECTORY_ENTRIES, b"");
        assert_error(&refused(&over), Status::Corrupted, "cfb.too_many_entries");
    }
}

#[test]
fn storages_may_nest_32_deep_and_not_deeper() {
    let nested = |version, depth| {
        let path = nested_path(depth);
        container(version, &[(&format!("{path}/leaf"), b"leaf")])
    };

    for version in [Version::V3, Version::V4] {
        let at_the_limit = nested(version, MAX_STORAGE_DEPTH);
        let mut file = open(&at_the_limit);
        // The stream sits in the deepest storage, which is 32 levels down.
        let leaf = format!("{}/leaf", nested_path(MAX_STORAGE_DEPTH));
        assert_eq!(
            file.read_stream(&leaf).unwrap().as_deref(),
            Some(&b"leaf"[..]),
            "{version:?}"
        );

        let over = nested(version, MAX_STORAGE_DEPTH + 1);
        assert_error(&refused(&over), Status::Corrupted, "cfb.too_deep");
    }
}

#[test]
fn a_storage_named_dot_counts_as_a_level() {
    // `Path` drops the `.` components of a path: the depth must not be read
    // from them.
    let path = nested_path(MAX_STORAGE_DEPTH + 1);
    let mut bytes = container(Version::V3, &[(&format!("{path}/leaf"), b"leaf")]);
    // The storages are the entries 1 to 33. Rename them: the name is UTF-16
    // and its first unit is the one to change.
    for index in 1..=MAX_STORAGE_DEPTH + 1 {
        let at = directory_entry_at(&bytes, index);
        put_u16(&mut bytes, at, u16::from(b'.'));
    }

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.too_deep");
}

#[test]
fn a_stream_may_claim_64_mb_and_not_one_byte_more() {
    let mut bytes = container(Version::V3, &[("big", &pattern(5000)), ("small", b"hi")]);
    let size_at = offset_of_entry(&bytes, "big") + STREAM_SIZE;

    // At the limit the size is accepted. The chain is shorter than the size
    // says, which the read reports.
    put_u64(&mut bytes, size_at, MAX_STREAM_SIZE);
    let error = open(&bytes).read_stream("big").unwrap_err();
    assert_error(&error, Status::Corrupted, "cfb.truncated");

    // One byte over: refused before any sector is read.
    put_u64(&mut bytes, size_at, MAX_STREAM_SIZE + 1);
    let mut file = open(&bytes);
    let error = file.read_stream("big").unwrap_err();
    assert_error(&error, Status::TooLarge, "cfb.stream_too_large");
    // The other streams stay readable.
    assert_eq!(
        file.read_stream("small").unwrap().as_deref(),
        Some(&b"hi"[..])
    );
}

#[test]
fn a_version_4_stream_may_claim_more_than_4_gb() {
    // The size is 64 bits wide from version 4 on.
    let mut bytes = container(Version::V4, &[("big", &pattern(5000))]);
    let size_at = offset_of_entry(&bytes, "big") + STREAM_SIZE;
    put_u64(&mut bytes, size_at, 5 * 1024 * 1024 * 1024);

    let error = open(&bytes).read_stream("big").unwrap_err();
    assert_error(&error, Status::TooLarge, "cfb.stream_too_large");
}

#[test]
fn a_stream_longer_than_its_chain_is_truncated() {
    let mut bytes = container(Version::V3, &[("short", &pattern(5000))]);
    let size_at = offset_of_entry(&bytes, "short") + STREAM_SIZE;
    put_u64(&mut bytes, size_at, 9000);

    let error = open(&bytes).read_stream("short").unwrap_err();
    assert_error(&error, Status::Corrupted, "cfb.truncated");
}

#[test]
fn an_empty_stream_reads_as_no_bytes_whatever_its_start_sector() {
    let mut bytes = container(Version::V3, &[("empty", b""), ("full", &pattern(5000))]);
    let start_at = offset_of_entry(&bytes, "empty") + START_SECTOR;
    put_u32(&mut bytes, start_at, 0x1234_5678);

    assert_eq!(open(&bytes).read_stream("empty").unwrap(), Some(Vec::new()));
}

#[test]
fn a_stream_chain_that_returns_to_its_start_is_a_loop() {
    let mut bytes = container(Version::V3, &[("loop", &pattern(6000)), ("ok", b"fine")]);
    let chain = chain_of(&bytes, "loop");
    assert!(chain.len() > 3, "{chain:?}");
    put_fat_next(&mut bytes, *chain.last().unwrap(), chain[0]);

    // The loop shows when the stream is read, and only for that stream.
    let mut file = open(&bytes);
    let error = file.read_stream("loop").unwrap_err();
    assert_error(&error, Status::Corrupted, "cfb.fat_loop");
    assert_eq!(
        file.read_stream("ok").unwrap().as_deref(),
        Some(&b"fine"[..])
    );
}

#[test]
fn a_stream_chain_that_joins_itself_is_a_loop() {
    let mut bytes = container(Version::V3, &[("loop", &pattern(6000))]);
    let chain = chain_of(&bytes, "loop");
    // The last sector points back to the third: that sector is pointed to
    // twice, which the crate refuses as it opens the container.
    put_fat_next(&mut bytes, *chain.last().unwrap(), chain[2]);

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.fat_loop");
}

#[test]
fn a_directory_chain_that_loops_is_a_loop() {
    // Ten streams and the root: three directory sectors in a version 3 file.
    let mut bytes = container_of_streams(Version::V3, 10, b"");
    let first = u32_at(&bytes, FIRST_DIRECTORY_SECTOR);
    let second = fat_next(&bytes, first);
    let third = fat_next(&bytes, second);
    assert_eq!(fat_next(&bytes, third), END_OF_CHAIN);
    put_fat_next(&mut bytes, third, first);

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.fat_loop");
}

#[test]
fn a_minifat_chain_that_loops_is_a_loop() {
    // 150 streams of one mini sector each: the MiniFAT takes two sectors in a
    // version 3 file, which hold 128 entries each.
    let mut bytes = container_of_streams(Version::V3, 150, &pattern(64));
    let first = u32_at(&bytes, FIRST_MINIFAT_SECTOR);
    let second = fat_next(&bytes, first);
    assert_eq!(fat_next(&bytes, second), END_OF_CHAIN);
    put_fat_next(&mut bytes, second, first);

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.fat_loop");
}

#[test]
fn a_chain_in_the_mini_stream_that_loops_is_a_loop() {
    // "loop" takes four mini sectors, in a row. The MiniFAT entry of the last
    // one points back to the first.
    let mut bytes = container(Version::V3, &[("loop", &pattern(200)), ("ok", b"fine")]);
    let first = u32_at(&bytes, offset_of_entry(&bytes, "loop") + START_SECTOR);
    let table = sector_at(&bytes, u32_at(&bytes, FIRST_MINIFAT_SECTOR));
    let entry = |bytes: &[u8], index: u32| u32_at(bytes, table + 4 * index as usize);
    assert_eq!(entry(&bytes, first + 2), first + 3);
    let last = table + 4 * (first + 3) as usize;
    put_u32(&mut bytes, last, first);

    // The loop shows when the stream is read, and only for that stream.
    let mut file = open(&bytes);
    let error = file.read_stream("loop").unwrap_err();
    assert_error(&error, Status::Corrupted, "cfb.fat_loop");
    assert_eq!(
        file.read_stream("ok").unwrap().as_deref(),
        Some(&b"fine"[..])
    );
}

#[test]
fn a_storage_that_is_its_own_ancestor_is_refused() {
    let mut bytes = container(Version::V3, &[("Dir/Stream", b"data")]);
    // The child of the storage is the root.
    let child_at = offset_of_entry(&bytes, "Dir") + CHILD;
    put_u32(&mut bytes, child_at, 0);

    assert_error(&refused(&bytes), Status::Corrupted, "cfb.directory");
}
