//! End-to-end tests for `git-along version` and `git along --version`.

mod common;

use assert_cmd::prelude::*;
use predicates::prelude::*;

#[test]
fn version_subcommand_matches_python_format() {
    common::git_along()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("git-along version "));
}

#[test]
fn version_flag_is_supported() {
    common::git_along().arg("--version").assert().success();
}
