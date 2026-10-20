// SPDX-License-Identifier: Apache-2.0
//! The errors of the container layer.

use macroport_core::container::ContainerError;
use macroport_core::report::Status;

#[test]
fn a_container_error_shows_its_code_then_its_message() {
    let error = ContainerError::new(
        Status::Corrupted,
        "cfb.fat_loop",
        "the FAT chain of a stream loops",
    );
    assert_eq!(error.status, Status::Corrupted);
    assert_eq!(
        error.to_string(),
        "cfb.fat_loop: the FAT chain of a stream loops"
    );
}
