// SPDX-License-Identifier: Apache-2.0
//! Reading streams by path.

use cfb::Version;

use crate::support::{container, open, pattern};

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
