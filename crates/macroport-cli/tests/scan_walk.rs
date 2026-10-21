// SPDX-License-Identifier: Apache-2.0
//! The walk of a scan, through the library: which files it covers, in which
//! order, and what it says of a path it cannot look at.

mod support;

use std::path::{Path, PathBuf};

use macroport_cli::scan::{Candidate, EXTENSIONS, report_path, walk};
use support::{put, scratch};

/// The paths of `candidates` relative to `dir`, as the report writes them.
fn relative_paths(dir: &Path, candidates: &[Candidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|candidate| {
            let inside = candidate
                .location
                .strip_prefix(dir)
                .expect("a candidate is below the directory that was walked");
            report_path(inside)
        })
        .collect()
}

#[test]
fn the_extensions_are_those_of_the_spec() {
    // SPEC M4: "Files considered: extensions `xlsm xltm xlam xlsb xls xlt xla
    // xlsx xltx bas cls frm`". Written out, so that the test does not repeat
    // the constant.
    assert_eq!(
        EXTENSIONS,
        [
            "xlsm", "xltm", "xlam", "xlsb", "xls", "xlt", "xla", "xlsx", "xltx", "bas", "cls",
            "frm"
        ]
    );
}

#[test]
fn a_directory_gives_the_files_with_a_covered_extension_in_any_case() {
    let dir = scratch("walk_covered");
    let mut expected = vec!["tree/folder.bas/inner.cls".to_owned()];
    put(&dir, "tree/folder.bas/inner.cls", b"x");
    for extension in EXTENSIONS {
        let lower = format!("tree/lower-{extension}.{extension}");
        let upper = format!("tree/UPPER-{extension}.{}", extension.to_uppercase());
        put(&dir, &lower, b"x");
        put(&dir, &upper, b"x");
        expected.extend([lower, upper]);
    }
    for ignored in [
        "tree/notes.txt",
        "tree/README",
        "tree/book.xls.bak",
        "tree/book.xlsx~",
        "tree/manifest.json",
        "tree/sub/.hidden",
    ] {
        put(&dir, ignored, b"x");
    }
    expected.sort();

    let found = walk(&[dir.join("tree")]);

    assert_eq!(relative_paths(&dir, &found), expected);
    assert!(found.iter().all(|candidate| candidate.unreadable.is_none()));
}

#[test]
fn a_name_that_is_only_an_extension_has_that_extension() {
    // The core takes the extension of `.xls` as `xls`; the walk agrees.
    let dir = scratch("walk_dotfiles");
    put(&dir, "tree/.xls", b"x");
    put(&dir, "tree/.BAS", b"x");
    put(&dir, "tree/.xlsm.txt", b"x");
    put(&dir, "tree/.txt", b"x");

    let found = walk(&[dir.join("tree")]);

    assert_eq!(relative_paths(&dir, &found), ["tree/.BAS", "tree/.xls"]);
}

#[test]
fn a_file_that_is_named_is_taken_whatever_its_extension() {
    let dir = scratch("walk_named");
    let notes = put(&dir, "notes.txt", b"x");
    let bare = put(&dir, "bare", b"x");

    let found = walk(&[notes, bare.clone()]);

    assert_eq!(relative_paths(&dir, &found), ["bare", "notes.txt"]);
    assert_eq!(found[0].location, bare);
    assert_eq!(found[0].path, report_path(&bare));
}

#[test]
fn the_files_come_in_the_order_of_their_paths_whatever_the_order_of_the_roots() {
    let dir = scratch("walk_order");
    for relative in ["b/2.bas", "a/1.bas", "a/10.bas", "a/2.bas", "a-b/1.bas"] {
        put(&dir, relative, b"x");
    }

    let forward = walk(&[dir.join("a"), dir.join("b"), dir.join("a-b")]);
    let backward = walk(&[dir.join("a-b"), dir.join("b"), dir.join("a")]);

    assert_eq!(forward, backward);
    // Byte order: "a-b/" sorts before "a/" since '-' is 0x2D and '/' 0x2F.
    assert_eq!(
        relative_paths(&dir, &forward),
        ["a-b/1.bas", "a/1.bas", "a/10.bas", "a/2.bas", "b/2.bas"]
    );
}

#[test]
fn a_file_that_is_reached_twice_is_taken_once() {
    let dir = scratch("walk_twice");
    let one = put(&dir, "tree/a/1.bas", b"x");
    put(&dir, "tree/2.bas", b"x");

    let found = walk(&[dir.join("tree"), one, dir.join("tree/a"), dir.join("tree")]);

    assert_eq!(relative_paths(&dir, &found), ["tree/2.bas", "tree/a/1.bas"]);
}

#[test]
fn a_file_reached_by_two_spellings_of_its_path_is_taken_once_under_the_shorter() {
    let dir = scratch("walk_spellings");
    put(&dir, "tree/a.bas", b"x");
    let detour = dir.join("tree/../tree");

    let forward = walk(&[dir.join("tree"), detour.clone()]);
    let backward = walk(&[detour, dir.join("tree")]);

    assert_eq!(forward, backward);
    assert_eq!(relative_paths(&dir, &forward), ["tree/a.bas"]);
}

#[cfg(unix)]
#[test]
fn a_file_reached_through_a_link_in_the_path_of_a_root_is_taken_once() {
    use std::os::unix::fs::symlink;

    let dir = scratch("walk_alias");
    put(&dir, "tree/a.bas", b"x");
    symlink(dir.join("tree"), dir.join("alias-of-the-tree")).expect("a link to the tree");

    let forward = walk(&[dir.join("tree"), dir.join("alias-of-the-tree")]);
    let backward = walk(&[dir.join("alias-of-the-tree"), dir.join("tree")]);

    assert_eq!(forward, backward);
    assert_eq!(relative_paths(&dir, &forward), ["tree/a.bas"]);
}

#[test]
fn a_trailing_separator_does_not_change_the_paths() {
    let dir = scratch("walk_separator");
    put(&dir, "tree/a.bas", b"x");
    let with = PathBuf::from(format!("{}/", dir.join("tree").display()));

    let found = walk(&[with]);

    assert_eq!(found, walk(&[dir.join("tree")]));
    assert!(!found[0].path.contains("//"));
}

#[test]
fn an_empty_directory_gives_no_file() {
    let dir = scratch("walk_empty");

    assert!(walk(&[dir]).is_empty());
}

#[test]
fn a_root_that_does_not_exist_comes_out_with_its_reason() {
    let dir = scratch("walk_missing");

    let found = walk(&[dir.join("missing")]);

    assert_eq!(found.len(), 1);
    assert_eq!(relative_paths(&dir, &found), ["missing"]);
    assert!(found[0].unreadable.is_some());
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_followed_except_for_a_root() {
    use std::os::unix::fs::symlink;

    let dir = scratch("walk_links");
    put(&dir, "real/a.bas", b"x");
    put(&dir, "tree/b.bas", b"x");
    symlink(dir.join("real/a.bas"), dir.join("tree/link.bas")).expect("a link to a file");
    symlink(dir.join("real"), dir.join("tree/linked-dir")).expect("a link to a directory");
    symlink(dir.join("nowhere.bas"), dir.join("tree/dangling.bas")).expect("a dangling link");
    symlink(dir.join("tree"), dir.join("root-link")).expect("a link to the root");

    let under_a_directory = walk(&[dir.join("tree")]);
    let under_a_link = walk(&[dir.join("root-link")]);

    assert_eq!(relative_paths(&dir, &under_a_directory), ["tree/b.bas"]);
    assert_eq!(relative_paths(&dir, &under_a_link), ["root-link/b.bas"]);
}

#[cfg(unix)]
#[test]
fn a_root_that_is_a_link_to_a_file_is_that_file() {
    use std::os::unix::fs::symlink;

    let dir = scratch("walk_file_link");
    let real = put(&dir, "real.dat", b"x");
    symlink(&real, dir.join("link")).expect("a link to a file");
    symlink(dir.join("nowhere"), dir.join("dangling")).expect("a dangling link");

    let to_a_file = walk(&[dir.join("link")]);
    let to_nothing = walk(&[dir.join("dangling")]);

    assert_eq!(relative_paths(&dir, &to_a_file), ["link"]);
    assert!(to_a_file[0].unreadable.is_none());
    assert_eq!(relative_paths(&dir, &to_nothing), ["dangling"]);
    assert!(to_nothing[0].unreadable.is_some());
}

#[cfg(unix)]
#[test]
fn a_root_that_is_neither_a_file_nor_a_directory_comes_out_with_its_reason() {
    let dir = scratch("walk_special");
    let pipe = dir.join("pipe");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(
        made.is_ok_and(|status| status.success()),
        "mkfifo makes a pipe"
    );

    let found = walk(&[pipe]);

    assert_eq!(relative_paths(&dir, &found), ["pipe"]);
    assert!(found[0].unreadable.is_some());
}

#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_listed_comes_out_with_its_reason() {
    let dir = scratch("walk_unlistable");
    put(&dir, "tree/ok.bas", b"x");
    put(&dir, "tree/locked/hidden.bas", b"x");
    let Some(_denied) = support::deny_access(&dir.join("tree/locked")) else {
        return;
    };

    let found = walk(&[dir.join("tree")]);

    assert_eq!(relative_paths(&dir, &found), ["tree/locked", "tree/ok.bas"]);
    assert!(found[0].unreadable.is_some());
    assert!(found[1].unreadable.is_none());
}
