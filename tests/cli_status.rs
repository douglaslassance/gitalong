//! End-to-end tests for `gitalong status`, `update` and `claim`, run once
//! per store backend.
//!
//! These exercise the binary against real origin, store and clone fixtures.
//! Heavier than the lib-level tests but they catch CLI-side wiring (argument
//! parsing, output formatting, exit codes).

mod common;

use assert_cmd::prelude::*;
use gitalong::for_each_store;
use gitalong::testing::Team;
use predicates::prelude::*;
use tempfile::{TempDir, tempdir};

/// Alice's clone with README committed and pushed, tracking uncommitted changes.
fn alice(team: &Team) -> TempDir {
    team.seeded_clone("Alice", &[("README", "hi")], |c| c.track_uncommitted = true)
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

for_each_store!(claim_on_unblocked_file_exits_zero, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    common::gitalong_in(m.path())
        .args(["claim", "README"])
        .assert()
        .success();
});
