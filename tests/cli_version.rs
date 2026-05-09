//! End-to-end tests for `gitalong version` and `gitalong --version`.

mod common;

use assert_cmd::prelude::*;
use predicates::prelude::*;

#[test]
fn version_subcommand_matches_python_format() {
    common::gitalong()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("gitalong version "));
}

#[test]
fn version_flag_is_supported() {
    common::gitalong().arg("--version").assert().success();
}
