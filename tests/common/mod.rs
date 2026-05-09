//! Shared helpers for integration tests.
//!
//! Each integration test gets its own `tempdir()` working tree so the tests
//! are independent and the host filesystem stays clean.

use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

/// Build a `Command` that runs the `gitalong` binary as if a user typed it.
pub fn gitalong() -> Command {
    Command::cargo_bin("gitalong").expect("gitalong binary not built")
}

/// Build a `Command` rooted at the given working directory.
pub fn gitalong_in(dir: &Path) -> Command {
    let mut cmd = gitalong();
    cmd.current_dir(dir);
    cmd
}
