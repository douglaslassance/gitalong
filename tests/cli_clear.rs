//! End-to-end tests for `gitalong clear`, run once per store backend.

mod common;

use assert_cmd::prelude::*;
use gitalong::for_each_store;
use gitalong::testing::Team;
use predicates::prelude::*;
use tempfile::TempDir;

/// Alice's clone with README committed and pushed, tracking uncommitted changes.
fn alice(team: &Team) -> TempDir {
    team.seeded_clone("Alice", &[("README", "hi")], |c| c.track_uncommitted = true)
}

/// The spread `status` prints for README in `dir`.
fn spread_of_readme(dir: &std::path::Path) -> String {
    let out = common::gitalong_in(dir)
        .args(["status", "README"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

for_each_store!(clear_drops_this_clones_records, |kind| {
    let team = Team::new(kind);
    let m = alice(&team);
    common::gitalong_in(m.path())
        .args(["claim", "README"])
        .assert()
        .success();
    assert_eq!(spread_of_readme(m.path()), "+-------");

    common::gitalong_in(m.path())
        .args(["clear"])
        .assert()
        .success();
    assert_eq!(spread_of_readme(m.path()), "-+-+----");
});

for_each_store!(
    clear_all_refuses_without_force_when_not_interactive,
    |kind| {
        let team = Team::new(kind);
        let m = alice(&team);
        common::gitalong_in(m.path())
            .args(["claim", "README"])
            .assert()
            .success();

        common::gitalong_in(m.path())
            .args(["clear", "--all"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("--force"));
        assert_eq!(
            spread_of_readme(m.path()),
            "+-------",
            "a refused clear must leave the records alone"
        );
    }
);

for_each_store!(clear_all_with_force_drops_every_clones_records, |kind| {
    let team = Team::new(kind);
    let alice = alice(&team);
    let bob = team.clone("Bob", |c| c.track_uncommitted = true);
    common::gitalong_in(alice.path())
        .args(["claim", "README"])
        .assert()
        .success();
    assert_eq!(spread_of_readme(bob.path()), "-------+");

    common::gitalong_in(bob.path())
        .args(["clear", "--all", "--force"])
        .assert()
        .success();
    assert_eq!(spread_of_readme(bob.path()), "-+-+----");
    assert_eq!(spread_of_readme(alice.path()), "-+-+----");
});
