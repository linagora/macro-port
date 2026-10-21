// SPDX-License-Identifier: Apache-2.0
//! Files that are not containers and containers that are broken: each is
//! refused with a typed error, never a panic.

use cfb::Version;
use macroport_core::container::CfbFile;
use macroport_core::report::Status;
use macroport_testkit::corpus;

use crate::support::{assert_error, assert_typed, container, put_u16, put_u32, refused};

fn malformed(name: &str) -> Vec<u8> {
    corpus::read(&format!("fixtures/malformed/{name}"))
}

#[test]
fn a_file_without_the_signature_is_refused() {
    for name in [
        "cfb-bad-signature.xls",
        "random-bytes.xls",
        "random-bytes.xlsm",
        "text-renamed.xlsm",
        "empty.xlsm",
    ] {
        let bytes = malformed(name);
        assert_error(&refused(&bytes), Status::Corrupted, "cfb.signature");
    }
}

#[test]
fn a_file_with_a_broken_fat_is_refused() {
    // The first DIFAT entry points far beyond the end of the file.
    let error = refused(&malformed("cfb-bad-fat.xls"));
    assert_error(&error, Status::Corrupted, "cfb.fat");
    assert!(error.message.contains("DIFAT"), "{error}");

    // A third of the file: the FAT lists sectors that are gone.
    let error = refused(&malformed("cfb-truncated.xls"));
    assert_error(&error, Status::Corrupted, "cfb.fat");
}

#[test]
fn no_malformed_file_panics_the_reader() {
    for name in corpus::files("fixtures/malformed", &[]) {
        let bytes = corpus::read(&name);
        match CfbFile::open(&bytes) {
            Ok(_) => {}
            Err(error) => assert_typed(&error),
        }
    }
}

#[test]
fn a_header_cut_short_is_refused() {
    let bytes = container(Version::V3, &[("a", b"a")]);
    for length in [8, 9, 100, 511] {
        assert_error(&refused(&bytes[..length]), Status::Corrupted, "cfb.header");
    }
    for length in [0, 1, 7] {
        assert_error(
            &refused(&bytes[..length]),
            Status::Corrupted,
            "cfb.signature",
        );
    }
}

#[test]
fn a_version_4_file_shorter_than_its_header_sector_is_refused() {
    let bytes = container(Version::V4, &[("a", b"a")]);
    assert_error(&refused(&bytes[..600]), Status::Corrupted, "cfb.header");
}

#[test]
fn the_values_of_the_header_are_checked() {
    let good = container(Version::V3, &[("a", b"a")]);
    crate::support::open(&good);

    // The major version, the byte order, the sector shift and the mini sector
    // shift.
    for (at, value) in [(0x1A, 5), (0x1C, 0xFEFF), (0x1E, 12), (0x20, 7)] {
        let mut bytes = good.clone();
        put_u16(&mut bytes, at, value);
        assert_error(&refused(&bytes), Status::Corrupted, "cfb.header");
    }

    // The mini stream cutoff.
    let mut bytes = good;
    put_u32(&mut bytes, 0x38, 1024);
    assert_error(&refused(&bytes), Status::Corrupted, "cfb.header");
}
