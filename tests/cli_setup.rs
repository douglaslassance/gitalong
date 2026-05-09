//! End-to-end tests for `gitalong setup`.

mod common;

use std::fs;
use std::path::Path;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

fn init_git_repo(path: &Path) {
    git2::Repository::init(path).unwrap();
}

#[test]
fn writes_minimum_config() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["setup", "https://example.com/store.git"])
        .assert()
        .success();

    let config_path = dir.path().join(".gitalong.json");
    assert!(config_path.is_file(), ".gitalong.json was not created");
    let body = fs::read_to_string(&config_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["store_url"], "https://example.com/store.git");
    assert_eq!(parsed["modify_permissions"], false);
    assert_eq!(parsed["track_uncommitted"], false);
    assert_eq!(parsed["pull_threshold"], 60.0);
}

#[test]
fn rejects_unrecognized_store_url() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["setup", "ftp://nope/somewhere"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid store URL"));

    // Failed setup should not have left a config file behind.
    assert!(!dir.path().join(".gitalong.json").exists());
}

#[test]
fn rejects_setup_outside_git_repo() {
    let dir = tempdir().unwrap();
    // No git init.
    common::gitalong_in(dir.path())
        .args(["setup", "https://example.com/store.git"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not in a git repository"));
}

#[test]
fn parses_store_headers_into_config() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://api.jsonbin.io/v3/b/abc",
            "-H",
            "X-Access-Key=secret",
            "-H",
            "X-Bin-Versioning=false",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(dir.path().join(".gitalong.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["store_headers"]["X-Access-Key"], "secret");
    assert_eq!(parsed["store_headers"]["X-Bin-Versioning"], "false");
}

#[test]
fn malformed_store_header_is_rejected() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://example.com/store.git",
            "-H",
            "missing-equals",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("KEY=VALUE"));
}

#[test]
fn update_gitignore_appends_patch() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    // Pre-existing content should be preserved.
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();

    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://example.com/store.git",
            "--update-gitignore",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(body.contains("build/"));
    assert!(body.contains("/.gitalong/"));
    assert!(body.contains("!/.gitalong.cfg"));
}

#[test]
fn update_gitignore_is_idempotent() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://example.com/store.git",
            "--update-gitignore",
        ])
        .assert()
        .success();
    let first = fs::read_to_string(dir.path().join(".gitignore")).unwrap();

    // Re-run setup: gitignore content must be unchanged.
    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://example.com/store.git",
            "--update-gitignore",
        ])
        .assert()
        .success();
    let second = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(first, second);
}

#[test]
fn update_hooks_installs_each_hook() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["setup", "https://example.com/store.git", "--update-hooks"])
        .assert()
        .success();

    let hooks = dir.path().join(".git/hooks");
    for name in [
        "post-applypatch",
        "post-checkout",
        "post-commit",
        "post-rewrite",
    ] {
        let p = hooks.join(name);
        assert!(p.is_file(), "hook {name} was not installed");
        let body = fs::read_to_string(&p).unwrap();
        assert!(
            body.contains("gitalong update"),
            "hook {name} should call `gitalong update`"
        );
    }
}

#[test]
fn modify_permissions_disables_core_filemode() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::gitalong_in(dir.path())
        .args([
            "setup",
            "https://example.com/store.git",
            "--modify-permissions",
        ])
        .assert()
        .success();

    // Read core.fileMode back from the on-disk git config.
    let repo = git2::Repository::discover(dir.path()).unwrap();
    let cfg = repo.config().unwrap();
    let file_mode: bool = cfg.get_bool("core.fileMode").unwrap();
    assert!(
        !file_mode,
        "modify_permissions should set core.fileMode=false"
    );
}
