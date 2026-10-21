// SPDX-License-Identifier: Apache-2.0
//! Containers cut short and containers with bytes changed: the reader gives
//! streams or a typed error, never a panic. (The fuzz targets of ticket #37
//! take over from these deterministic runs.)

use cfb::Version;
use macroport_core::container::{CfbFile, MAX_STREAM_SIZE};
use macroport_core::report::Status;
use macroport_testkit::corpus;

use crate::fixtures::reference_streams;
use crate::support::{assert_typed, container, pattern};

/// How many times each container is changed, with a fresh set of bytes.
const MUTATIONS: usize = 1_000;

/// A container that holds streams of every kind: in the mini stream, in
/// regular sectors, in a storage.
fn built(version: Version) -> Vec<u8> {
    let big = pattern(9_000);
    container(
        version,
        &[
            ("Workbook", &big),
            ("Dir/Stream", b"stream"),
            ("Top", b"top"),
        ],
    )
}

/// The paths of the streams of a container that is well-formed.
fn paths_of(bytes: &[u8]) -> Vec<String> {
    let streams = reference_streams(bytes);
    streams.into_iter().map(|(path, _)| path).collect()
}

/// Opens `bytes` and reads these streams: a failure must be a typed one.
fn exercise(bytes: &[u8], paths: &[String]) {
    let mut file = match CfbFile::open(bytes) {
        Ok(file) => file,
        Err(error) => return assert_typed(&error),
    };
    for path in paths {
        file.has_storage(path);
        // The directory gives a length exactly where there is a stream.
        let claimed = file.stream_len(path);
        assert_eq!(claimed.is_some(), file.has_stream(path), "{path}");
        // Such a stream is refused before a byte is read, whatever its chain.
        let above_cap = claimed.is_some_and(|len| len > MAX_STREAM_SIZE);
        match file.read_stream(path) {
            // A read that succeeds gives as many bytes as the directory says.
            Ok(Some(stream)) => {
                assert!(!above_cap, "{path}");
                assert_eq!(claimed, Some(stream.len() as u64), "{path}");
            }
            Ok(None) => assert_eq!(claimed, None, "{path}"),
            // A size that the container only claims, or a read that cannot
            // reserve what the chain holds.
            Err(error) if error.status == Status::TooLarge => {
                assert_eq!(error.code, "cfb.stream_too_large", "{error}");
            }
            Err(error) => {
                assert!(!above_cap, "{path}: {error}");
                assert_typed(&error);
            }
        }
    }
}

/// A xorshift generator, so that every run changes the same bytes.
struct Noise(u64);

impl Noise {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % bound as u64).unwrap()
    }
}

/// `bytes` with one to eight of them changed. Half of the changes land in the
/// first 4,096 bytes, where the header sits.
fn mutated(bytes: &[u8], noise: &mut Noise) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    for _ in 0..=noise.below(8) {
        let reach = if noise.below(2) == 0 {
            4096
        } else {
            bytes.len()
        };
        let at = noise.below(reach.min(bytes.len()));
        bytes[at] = match noise.below(3) {
            0 => 0x00,
            1 => 0xFF,
            _ => u8::try_from(noise.below(256)).unwrap(),
        };
    }
    bytes
}

fn survives_changes(bytes: &[u8], seed: u64) {
    let paths = paths_of(bytes);
    let mut noise = Noise(seed);
    for _ in 0..MUTATIONS {
        exercise(&mutated(bytes, &mut noise), &paths);
    }
}

#[test]
fn a_container_cut_short_never_panics_the_reader() {
    // Every length of a version 3 file; every seventh of a version 4 file.
    for (version, step) in [(Version::V3, 1), (Version::V4, 7)] {
        let bytes = built(version);
        let paths = paths_of(&bytes);
        for length in (0..bytes.len()).step_by(step) {
            exercise(&bytes[..length], &paths);
        }
    }
}

#[test]
fn a_container_with_bytes_changed_never_panics_the_reader() {
    survives_changes(&built(Version::V3), 1);
    survives_changes(&built(Version::V4), 2);
}

#[test]
fn a_workbook_with_bytes_changed_never_panics_the_reader() {
    // A workbook with a VBA project, one with an XLM sheet, and an encrypted
    // package, which is a container of its own kind.
    let names = [
        "01-01-hide-unhide-columns-find.xls",
        "xlm-01-macrosheet.xls",
        "enc-01-open-password.xlsm",
    ];
    for (seed, name) in (3..).zip(names) {
        survives_changes(&corpus::read(&format!("fixtures/generated/{name}")), seed);
    }
}
