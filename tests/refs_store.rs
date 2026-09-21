//! The refs store leaves nothing on the server but metadata under a hidden
//! namespace, and nothing in the working tree.

mod common;

use std::fs;

use assert_cmd::prelude::*;
use gitalong::testing::{StoreKind, Team, git};
use tempfile::tempdir;

#[test]
fn publishes_only_metadata_under_a_hidden_ref() {
    let team = Team::new(StoreKind::Refs);
    let alice = tempdir().unwrap();
    git(
        alice.path(),
        &[
            "clone",
            "--quiet",
            team.origin().to_str().unwrap(),
            alice.path().to_str().unwrap(),
        ],
    );
    git(alice.path(), &["config", "user.email", "alice@example.com"]);
    git(alice.path(), &["config", "user.name", "Alice"]);
    let mut setup = team.setup_args();
    setup.push("--track-uncommitted".to_string());
    common::gitalong_in(alice.path())
        .args(&setup)
        .assert()
        .success();
    fs::write(alice.path().join("shared.txt"), b"shared").unwrap();
    git(alice.path(), &["add", "--all"]);
    git(
        alice.path(),
        &["commit", "--quiet", "-m", "Add gitalong config"],
    );
    git(alice.path(), &["push", "--quiet", "-u", "origin", "main"]);

    fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
    common::gitalong_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    let refs = git(
        team.origin(),
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/gitalong/v1/",
        ],
    );
    let oids: Vec<&str> = refs.lines().collect();
    assert_eq!(oids.len(), 1, "one clone published, one ref expected");
    assert_eq!(
        git(
            team.origin(),
            &["rev-parse", &format!("{}^{{tree}}", oids[0])]
        ),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        "records must hang off the empty tree"
    );
    assert!(!git(team.origin(), &["branch", "--list"]).contains("gitalong"));
    assert!(!git(alice.path(), &["branch", "--all"]).contains("gitalong"));
    assert!(
        !alice.path().join(".gitalong").exists(),
        "the refs store keeps nothing in the working tree"
    );
}
