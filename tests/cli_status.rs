//! End-to-end tests for `gitalong status`, `update` and `claim`, run once
//! per store backend.
//!
//! These exercise the binary against real origin, store and clone fixtures.
//! Heavier than the lib-level tests but they catch CLI-side wiring (argument
//! parsing, output formatting, exit codes).

mod common;

use std::fs;

use assert_cmd::prelude::*;
use gitalong::for_each_store;
use gitalong::testing::{Team, git};
use predicates::prelude::*;
use tempfile::{TempDir, tempdir};

/// Alice's clone with README committed and pushed, tracking uncommitted changes.
fn alice(team: &Team) -> TempDir {
    team.seeded_clone("Alice", &[("README", "hi")], |c| c.track_uncommitted = true)
}

/// Run the binary in `dir` and parse its stdout as JSON, asserting success.
fn json_of(dir: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let out = common::gitalong_in(dir).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} failed");
    serde_json::from_slice(&out.stdout).unwrap()
}

for_each_store!(update_succeeds_in_a_managed_repo, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    common::gitalong_in(m.path())
        .args(["update"])
        .assert()
        .success();
});

#[test]
fn update_fails_outside_a_managed_repo() {
    let dir = tempdir().unwrap();
    common::gitalong_in(dir.path())
        .args(["update"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not in a managed repository"));
}

for_each_store!(status_renders_one_line_per_file, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    common::gitalong_in(m.path())
        .args(["status", "README", "missing.txt"])
        .assert()
        .success()
        .stdout(predicate::function(|s: &str| {
            // First line refers to README, second to missing.txt; both follow
            // the eight-character spread + filename layout.
            let lines: Vec<&str> = s.lines().collect();
            lines.len() == 2
                && lines[0].starts_with(['+', '-'])
                && lines[1].starts_with("-------- missing.txt")
        }));
});

for_each_store!(status_json_is_one_array_in_input_order, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    let parsed = json_of(m.path(), &["status", "--json", "README", "missing.txt"]);
    let rows = parsed.as_array().unwrap();
    assert_eq!(rows.len(), 2);

    assert_eq!(rows[0]["filename"], "README");
    assert!(rows[0]["commit"]["sha"].is_string());
    assert_eq!(rows[0]["commit"]["author"], "Alice");
    assert_eq!(rows[0]["spread"].as_str().unwrap().len(), 8);
    assert!(rows[0]["blocked"].is_null());

    assert_eq!(rows[1]["filename"], "missing.txt");
    assert!(rows[1]["commit"].is_null());
    assert_eq!(rows[1]["spread"], "--------");
    assert_eq!(rows[1]["flags"].as_array().unwrap().len(), 0);
});

for_each_store!(status_json_survives_a_filename_with_spaces, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    fs::write(m.path().join("my file.txt"), b"hi").unwrap();
    git(m.path(), &["add", "my file.txt"]);
    git(m.path(), &["commit", "-m", "spaced"]);

    let parsed = json_of(m.path(), &["status", "--json", "my file.txt"]);
    assert_eq!(parsed[0]["filename"], "my file.txt");
    assert!(parsed[0]["commit"]["sha"].is_string());
});

for_each_store!(claim_json_reports_blocked_per_file, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    let parsed = json_of(m.path(), &["claim", "--json", "README"]);
    assert_eq!(parsed[0]["filename"], "README");
    assert_eq!(parsed[0]["blocked"], false);
});

for_each_store!(claim_on_unblocked_file_exits_zero, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    common::gitalong_in(m.path())
        .args(["claim", "README"])
        .assert()
        .success();
});
