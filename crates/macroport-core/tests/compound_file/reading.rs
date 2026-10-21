// SPDX-License-Identifier: Apache-2.0
//! Reading streams by path.

use cfb::Version;
use macroport_core::container::MAX_STREAM_SIZE;

use crate::support::{STREAM_SIZE, container, offset_of_entry, open, pattern, put_u64};

#[test]
fn streams_read_back_whatever_their_size() {
    // Under 4,096 bytes a stream lives in the mini stream, from there in
    // regular sectors.
    let sizes = [0, 1, 63, 64, 65, 4095, 4096, 4097, 100_000];
    for version in [Version::V3, Version::V4] {
        let data: Vec<Vec<u8>> = sizes.iter().map(|&size| pattern(size)).collect();
        let names: Vec<String> = sizes.iter().map(|size| format!("stream{size}")).collect();
        let streams: Vec<(&str, &[u8])> = names
            .iter()
            .zip(&data)
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        let bytes = container(version, &streams);

        let mut file = open(&bytes);
        for (name, data) in &streams {
            assert_eq!(
                file.read_stream(name).unwrap().as_deref(),
                Some(*data),
                "{name} in {version:?}"
            );
        }
    }
}

#[test]
fn paths_compare_without_regard_to_case() {
    let bytes = container(
        Version::V3,
        &[
            ("Workbook", b"workbook"),
            ("_VBA_PROJECT_CUR/VBA/dir", b"dir"),
            ("Éa", b"accent"),
        ],
    );
    let mut file = open(&bytes);

    let cases: [(&str, &[u8]); 8] = [
        ("Workbook", b"workbook"),
        ("wOrKbOoK", b"workbook"),
        ("/WORKBOOK", b"workbook"),
        ("_VBA_PROJECT_CUR/VBA/dir", b"dir"),
        ("/_Vba_Project_Cur/VBA/Dir", b"dir"),
        // Beyond ASCII, as the case mapping of MS-CFB has it.
        ("Éa", b"accent"),
        ("éA", b"accent"),
        ("ÉA", b"accent"),
    ];
    for (path, data) in cases {
        let found = file.read_stream(path).unwrap();
        assert_eq!(found.as_deref(), Some(data), "{path}");
    }
}

#[test]
fn a_path_that_names_no_stream_reads_as_none() {
    let bytes = container(Version::V3, &[("Dir/Stream", b"data"), ("Top", b"top")]);
    let mut file = open(&bytes);

    let paths = [
        "Missing",
        "Dir/Missing",
        "Missing/Stream",
        "Top/Below",
        // A storage is not a stream, and nor is the root.
        "Dir",
        "",
        "/",
        // Paths that no container can hold.
        "Dir//Stream",
        "Dir/Stream/",
        ".",
        "Dir/../Top",
        "./Top",
        "Dir\\Stream",
        "C:Top",
        "Top!",
        "a\u{0}b",
        &"x".repeat(100),
    ];
    for path in paths {
        assert_eq!(file.read_stream(path).unwrap(), None, "{path:?}");
        assert!(!file.has_stream(path), "{path:?}");
        assert_eq!(file.stream_len(path), None, "{path:?}");
    }
}

#[test]
fn the_length_of_a_stream_is_the_one_that_the_directory_claims() {
    let mut bytes = container(
        Version::V3,
        &[
            ("Dir/Empty", b""),
            ("Dir/Short", b"data"),
            ("Big", &pattern(5000)),
        ],
    );
    {
        let file = open(&bytes);
        assert_eq!(file.stream_len("Dir/Empty"), Some(0));
        assert_eq!(file.stream_len("DIR/short"), Some(4));
        assert_eq!(file.stream_len("/Big"), Some(5000));
    }

    // Nothing is read to check it: a claim that the chain cannot back, or
    // that a read refuses, is told as it is.
    let size_at = offset_of_entry(&bytes, "Big") + STREAM_SIZE;
    for claimed in [9000, MAX_STREAM_SIZE, MAX_STREAM_SIZE + 1] {
        put_u64(&mut bytes, size_at, claimed);

        assert_eq!(open(&bytes).stream_len("Big"), Some(claimed));
    }
}

#[test]
fn storages_and_streams_are_told_apart() {
    let bytes = container(Version::V3, &[("Dir/Stream", b"data")]);
    let file = open(&bytes);

    assert!(file.has_storage(""));
    assert!(file.has_storage("dir"));
    assert!(!file.has_stream("dir"));
    assert!(file.has_stream("DIR/STREAM"));
    assert!(!file.has_storage("dir/stream"));
    assert!(!file.has_storage("dir//stream"));
}

#[test]
fn an_encrypted_package_holds_both_of_its_streams_at_the_root() {
    let cases: [(&[&str], bool); 6] = [
        (&["EncryptionInfo", "EncryptedPackage"], true),
        (&["encryptioninfo", "ENCRYPTEDPACKAGE"], true),
        (&["EncryptionInfo"], false),
        (&["EncryptedPackage"], false),
        (&["Dir/EncryptionInfo", "Dir/EncryptedPackage"], false),
        (&["Workbook"], false),
    ];
    for (paths, expected) in cases {
        let streams: Vec<(&str, &[u8])> = paths.iter().map(|&path| (path, &b"data"[..])).collect();
        let bytes = container(Version::V3, &streams);

        assert_eq!(open(&bytes).is_encrypted_package(), expected, "{paths:?}");
    }
}
