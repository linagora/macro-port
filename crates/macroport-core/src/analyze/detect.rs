// SPDX-License-Identifier: Apache-2.0
//! What a file is, from its name.

use crate::report::Container;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
