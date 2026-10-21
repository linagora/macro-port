// SPDX-License-Identifier: Apache-2.0
//! What a file is, from its first bytes and its name: the bytes decide how it
//! is read, the extension picks the variant of the format within its family.

use crate::report::Container;

/// The signatures of a ZIP archive: a local file header, an empty archive and
/// a spanned one.
const ZIP_SIGNATURES: [&[u8]; 3] = [b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"];

/// The signature that opens a Compound File Binary container.
const COMPOUND_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Whether the first bytes of a file are those of a ZIP archive, the container
/// of the OOXML formats.
pub(super) fn is_zip(bytes: &[u8]) -> bool {
    (ZIP_SIGNATURES.iter()).any(|signature| bytes.starts_with(signature))
}

/// Whether the first bytes of a file are those of a compound file: the
/// container of the `.xls` workbooks and of the encrypted OOXML packages.
pub(super) fn is_compound(bytes: &[u8]) -> bool {
    bytes.starts_with(&COMPOUND_SIGNATURE)
}

/// The extension of the file that `path` names, in lowercase and without the
/// dot; empty if the file has none. Both `/` and `\` separate the names of a
/// path.
pub(super) fn extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The container that an extension names: one of the Excel formats, or
/// `Unknown`.
pub(super) fn container_by_extension(extension: &str) -> Container {
    match extension {
        "xlsm" => Container::Xlsm,
        "xltm" => Container::Xltm,
        "xlam" => Container::Xlam,
        "xlsb" => Container::Xlsb,
        "xls" => Container::Xls,
        "xlt" => Container::Xlt,
        "xla" => Container::Xla,
        "xlsx" => Container::Xlsx,
        "xltx" => Container::Xltx,
        _ => Container::Unknown,
    }
}

/// The container of an OOXML package, or of an encrypted one: the format its
/// extension names, or `Unknown` when the extension names another family.
pub(super) fn ooxml_container(extension: &str) -> Container {
    match container_by_extension(extension) {
        Container::Xls | Container::Xlt | Container::Xla => Container::Unknown,
        container => container,
    }
}

/// The container of a BIFF8 workbook: the format its extension names, or
/// `Unknown` when the extension names another family.
pub(super) fn biff_container(extension: &str) -> Container {
    match container_by_extension(extension) {
        container @ (Container::Xls | Container::Xlt | Container::Xla) => container,
        _ => Container::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_bytes_tell_a_zip_archive() {
        for zip in [&b"PK\x03\x04rest"[..], b"PK\x05\x06", b"PK\x07\x08"] {
            assert!(is_zip(zip), "{zip:?}");
        }
        for other in [&b""[..], b"PK", b"PK\x01\x02", b"MZ", b"\xD0\xCF\x11\xE0"] {
            assert!(!is_zip(other), "{other:?}");
        }
    }

    #[test]
    fn the_first_bytes_tell_a_compound_file() {
        let signature = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

        assert!(is_compound(signature));
        assert!(is_compound(&[&signature[..], b"rest"].concat()));
        for other in [
            &b""[..],
            &signature[..7],
            b"PK\x03\x04",
            b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE0",
        ] {
            assert!(!is_compound(other), "{other:?}");
        }
    }

    #[test]
    fn the_extension_is_that_of_the_last_name_in_lowercase() {
        for (path, extension_of_path) in [
            ("a.xlsm", "xlsm"),
            ("dir/Sub.Dir/Book.XLSM", "xlsm"),
            ("C:\\Users\\a.b\\Book.Xls", "xls"),
            ("archive.tar.gz", "gz"),
            ("noextension", ""),
            ("dir.d/noextension", ""),
            ("dir.d\\noextension", ""),
            ("trailing.", ""),
            ("", ""),
        ] {
            assert_eq!(extension(path), extension_of_path, "{path}");
        }
    }

    #[test]
    fn an_extension_names_a_container_or_none() {
        assert_eq!(container_by_extension("xltm"), Container::Xltm);
        assert_eq!(container_by_extension("xlsb"), Container::Xlsb);
        assert_eq!(container_by_extension("txt"), Container::Unknown);
        assert_eq!(container_by_extension(""), Container::Unknown);
    }

    #[test]
    fn a_family_of_formats_takes_the_extensions_of_its_formats_only() {
        assert_eq!(ooxml_container("xlsm"), Container::Xlsm);
        assert_eq!(ooxml_container("xlsb"), Container::Xlsb);
        assert_eq!(ooxml_container("xla"), Container::Unknown);
        assert_eq!(biff_container("xlt"), Container::Xlt);
        assert_eq!(biff_container("xls"), Container::Xls);
        assert_eq!(biff_container("xlsx"), Container::Unknown);
        assert_eq!(biff_container(""), Container::Unknown);
    }
}
