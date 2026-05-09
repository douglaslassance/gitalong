//! End-to-end lifecycle test mirroring the Python 0.x `cases.test_lib`.
//!
//! Runs the full team-collaboration scenario top-to-bottom:
//!
//! 1. Two clones share an origin repo and a gitalong store.
//! 2. Alice sets up gitalong (with `--track-uncommitted`,
//!    `--update-gitignore`, and `--update-hooks`), commits her config, and
//!    pushes.
//! 3. Bob clones origin and runs `gitalong update` — the store now has
//!    nothing because Alice has nothing in flight.
//! 4. Alice creates a local-only commit and edits an uncommitted file, then
//!    runs `update`.
//! 5. Bob runs `status` against those paths and sees Alice as the holder.
//! 6. Bob's `claim` against the same paths is blocked by Alice's records.
//! 7. Alice's claim of an unrelated file succeeds.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} in {} failed: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn full_lifecycle_two_clones() {
    // Standalone bare repos for code (origin) and gitalong's commit ledger (store).
    let store = tempdir().unwrap();
    run(store.path(), &["init", "--bare", "--initial-branch=main"]);
    let origin = tempdir().unwrap();
    run(origin.path(), &["init", "--bare", "--initial-branch=main"]);

    let store_url = format!("file://{}", store.path().display());
    let origin_url = origin.path().to_str().unwrap().to_string();

    // ---- Alice ----
    let alice = tempdir().unwrap();
    run(
        alice.path(),
        &["clone", &origin_url, alice.path().to_str().unwrap()],
    );
    run(alice.path(), &["config", "user.email", "alice@example.com"]);
    run(alice.path(), &["config", "user.name", "Alice"]);

    // `gitalong setup` writes the config, the .gitignore patch, and the hooks.
    common::gitalong_in(alice.path())
        .args([
            "setup",
            &store_url,
            "--track-uncommitted",
            "--tracked-extensions",
            ".txt",
            "--update-gitignore",
            "--update-hooks",
        ])
        .assert()
        .success();

    // Commit the freshly-created config and the gitignore so Alice's clone
    // is in a clean baseline state.
    fs::write(alice.path().join("README"), b"hello").unwrap();
    run(
        alice.path(),
        &["add", "README", ".gitalong.json", ".gitignore"],
    );
    run(alice.path(), &["commit", "-m", "init"]);
    run(alice.path(), &["push", "-u", "origin", "main"]);

    // First update: nothing to track yet — clean push, clean working tree.
    common::gitalong_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    // ---- Bob ----
    let bob = tempdir().unwrap();
    run(
        bob.path(),
        &["clone", &origin_url, bob.path().to_str().unwrap()],
    );
    run(bob.path(), &["config", "user.email", "bob@example.com"]);
    run(bob.path(), &["config", "user.name", "Bob"]);

    // ---- Alice creates work ----
    fs::write(alice.path().join("local.txt"), b"alice-local").unwrap();
    run(alice.path(), &["add", "local.txt"]);
    run(alice.path(), &["commit", "-m", "alice's local commit"]);
    fs::write(alice.path().join("draft.txt"), b"alice-draft").unwrap();
    common::gitalong_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    // ---- Bob's view of those files ----
    common::gitalong_in(bob.path())
        .args(["status", "local.txt", "draft.txt"])
        .assert()
        .success()
        .stdout(predicate::function(|s: &str| {
            // Both lines exist, both lead with a non-empty spread (some `+`).
            let lines: Vec<&str> = s.lines().collect();
            lines.len() == 2 && lines.iter().all(|l| l.contains('+'))
        }));

    // ---- Bob's claim is blocked ----
    common::gitalong_in(bob.path())
        .args(["claim", "draft.txt"])
        .assert()
        .failure();

    // ---- Alice claims an unrelated path successfully ----
    common::gitalong_in(alice.path())
        .args(["claim", "README"])
        .assert()
        .success();
}
