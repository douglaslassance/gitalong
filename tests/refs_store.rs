//! End-to-end test of the default refs store: two clones share nothing but
//! their origin, and claims still flow between them.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn claims_flow_through_refs_on_origin() {
    let origin = tempdir().unwrap();
    git(
        origin.path(),
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    let origin_url = origin.path().to_str().unwrap().to_string();

    let alice = tempdir().unwrap();
    git(
        alice.path(),
        &[
            "clone",
            "--quiet",
            &origin_url,
            alice.path().to_str().unwrap(),
        ],
    );
    git(alice.path(), &["config", "user.email", "alice@example.com"]);
    git(alice.path(), &["config", "user.name", "Alice"]);
    common::gitalong_in(alice.path())
        .args(["setup", "--pull-threshold", "0", "--track-uncommitted"])
        .assert()
        .success();
    fs::write(alice.path().join("shared.txt"), b"shared").unwrap();
    git(alice.path(), &["add", "-A"]);
    git(
        alice.path(),
        &["commit", "--quiet", "-m", "Add gitalong config"],
    );
    git(alice.path(), &["push", "--quiet", "-u", "origin", "main"]);

    let bob = tempdir().unwrap();
    git(
        bob.path(),
        &[
            "clone",
            "--quiet",
            &origin_url,
            bob.path().to_str().unwrap(),
        ],
    );

    // ---- Alice edits without committing and publishes ----
    fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
    common::gitalong_in(alice.path())
        .args(["update"])
        .assert()
        .success();

    // ---- Bob sees the file held elsewhere and is blocked ----
    common::gitalong_in(bob.path())
        .args(["status", "shared.txt"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("-------+ shared.txt"));
    common::gitalong_in(bob.path())
        .args(["claim", "shared.txt"])
        .assert()
        .failure();

    // ---- Only metadata reached origin ----
    let refs = git(
        origin.path(),
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
            origin.path(),
            &["rev-parse", &format!("{}^{{tree}}", oids[0])]
        ),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        "records must hang off the empty tree"
    );
    assert!(!git(origin.path(), &["branch", "--list"]).contains("gitalong"));
    assert!(!git(bob.path(), &["branch", "--all"]).contains("gitalong"));
    assert!(
        !alice.path().join(".gitalong").exists(),
        "the refs store keeps nothing in the working tree"
    );
}
