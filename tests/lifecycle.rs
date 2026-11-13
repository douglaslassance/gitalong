//! End-to-end lifecycle test mirroring the Python 0.x `cases.test_lib`, run
//! once per store backend.
//!
//! Runs the full team-collaboration scenario top-to-bottom:
//!
//! 1. Two clones share an origin repo and a Git Along store.
//! 2. Alice sets up Git Along (with `--track-uncommitted` and
//!    `--update-hooks`), commits her config, and pushes.
//! 3. Bob clones origin and runs `git along update` — the store now has
//!    nothing because Alice has nothing in flight.
//! 4. Alice creates a local-only commit and edits an uncommitted file, then
//!    runs `update`.
//! 5. Bob runs `status` against those paths and sees Alice as the holder.
//! 6. Bob's `claim` against either path is blocked by Alice's records.
//! 7. Alice pushes and Bob pulls; Bob's claim of the committed file succeeds.
//! 8. Alice's claim of an unrelated file succeeds.

mod common;

use std::fs;

use assert_cmd::prelude::*;
use git_along::for_each_store;
use git_along::testing::{Team, git};
use predicates::prelude::*;
use tempfile::tempdir;

for_each_store!(full_lifecycle_two_clones, |kind| {
    let team = Team::new(kind);
    let origin_url = team.origin().to_str().unwrap().to_string();

    let alice = tempdir().unwrap();
    git(
        alice.path(),
        &["clone", &origin_url, alice.path().to_str().unwrap()],
    );
    git(alice.path(), &["config", "user.email", "alice@example.com"]);
    git(alice.path(), &["config", "user.name", "Alice"]);

    // `git along setup` writes the config and the hooks.
    let mut setup = team.setup_args();
    setup.extend(
        [
            "--track-uncommitted",
            "--tracked-extensions",
            ".txt",
            "--update-hooks",
        ]
        .map(String::from),
    );
    common::git_along_in(alice.path())
        .args(&setup)
        .assert()
        .success();

    // Commit the freshly-created config so Alice's clone
    // is in a clean baseline state.
    fs::write(alice.path().join("README"), b"hello").unwrap();
    git(alice.path(), &["add", "README", ".alongconfig"]);
    git(alice.path(), &["commit", "-m", "init"]);
    git(alice.path(), &["push", "-u", "origin", "main"]);

    // First update: nothing to track yet — clean push, clean working tree.
    common::git_along_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    let bob = tempdir().unwrap();
    git(
        bob.path(),
        &["clone", &origin_url, bob.path().to_str().unwrap()],
    );
    git(bob.path(), &["config", "user.email", "bob@example.com"]);
    git(bob.path(), &["config", "user.name", "Bob"]);

    // ---- Alice creates work ----
    fs::write(alice.path().join("local.txt"), b"alice-local").unwrap();
    git(alice.path(), &["add", "local.txt"]);
    git(alice.path(), &["commit", "-m", "alice's local commit"]);
    fs::write(alice.path().join("draft.txt"), b"alice-draft").unwrap();
    common::git_along_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    // ---- Bob's view of those files ----
    common::git_along_in(bob.path())
        .args(["status", "local.txt", "draft.txt"])
        .assert()
        .success()
        .stdout(predicate::function(|s: &str| {
            // Both lines exist, both lead with a non-empty spread (some `+`).
            let lines: Vec<&str> = s.lines().collect();
            lines.len() == 2 && lines.iter().all(|l| l.contains('+'))
        }));

    // ---- Bob's claims are blocked, by the uncommitted edit and the unpushed commit alike ----
    common::git_along_in(bob.path())
        .args(["claim", "draft.txt"])
        .assert()
        .failure();
    common::git_along_in(bob.path())
        .args(["claim", "local.txt"])
        .assert()
        .failure();

    // ---- Once Alice pushes and Bob pulls, the commit no longer blocks ----
    git(alice.path(), &["push"]);
    git(bob.path(), &["pull", "--ff-only"]);
    common::git_along_in(bob.path())
        .args(["claim", "local.txt"])
        .assert()
        .success();

    // ---- Alice claims an unrelated path successfully ----
    common::git_along_in(alice.path())
        .args(["claim", "README"])
        .assert()
        .success();
});
