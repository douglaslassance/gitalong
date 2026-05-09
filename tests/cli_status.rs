//! End-to-end tests for `gitalong status` and `gitalong update`.
//!
//! These exercise the binary against real bare-store + bare-origin + managed
//! clone fixtures. Heavier than the lib-level tests but they catch CLI-side
//! wiring (argument parsing, output formatting, exit codes).

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

const GITIGNORE_PATCH: &str = "# Gitalong\n/.gitalong/\n!/.gitalong.cfg\n";

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Stand up a complete fixture and return paths for store, origin, managed.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
    let store = tempdir().unwrap();
    run(store.path(), &["init", "--bare", "--initial-branch=main"]);
    let origin = tempdir().unwrap();
    run(origin.path(), &["init", "--bare", "--initial-branch=main"]);

    let managed = tempdir().unwrap();
    run(
        managed.path(),
        &[
            "clone",
            origin.path().to_str().unwrap(),
            managed.path().to_str().unwrap(),
        ],
    );
    run(managed.path(), &["config", "user.email", "alice@example.com"]);
    run(managed.path(), &["config", "user.name", "Alice"]);

    let cfg = format!(
        r#"{{"store_url":"file://{}","store_headers":{{}},"modify_permissions":false,"track_binaries":false,"tracked_extensions":[],"pull_threshold":0.0,"track_uncommitted":true}}"#,
        store.path().display()
    );
    fs::write(managed.path().join(".gitalong.json"), cfg).unwrap();
    fs::write(managed.path().join(".gitignore"), GITIGNORE_PATCH).unwrap();
    fs::write(managed.path().join("README"), b"hi").unwrap();
    run(
        managed.path(),
        &["add", "README", ".gitalong.json", ".gitignore"],
    );
    run(managed.path(), &["commit", "-m", "init"]);
    run(managed.path(), &["push", "-u", "origin", "main"]);

    (store, origin, managed)
}

#[test]
fn update_succeeds_in_a_managed_repo() {
    let (_s, _o, m) = fixture();
    common::gitalong_in(m.path())
        .args(["update"])
        .assert()
        .success();
}

#[test]
fn update_fails_outside_a_managed_repo() {
    let dir = tempdir().unwrap();
    common::gitalong_in(dir.path())
        .args(["update"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not in a managed repository"));
}

#[test]
fn status_renders_one_line_per_file() {
    let (_s, _o, m) = fixture();

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
}

#[test]
fn claim_on_unblocked_file_exits_zero() {
    let (_s, _o, m) = fixture();
    common::gitalong_in(m.path())
        .args(["claim", "README"])
        .assert()
        .success();
}
