//! End-to-end tests for `git along setup`.

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

    common::git_along_in(dir.path())
        .args(["setup", "https://example.com/store.git"])
        .assert()
        .success();

    let config_path = dir.path().join(".alongconfig");
    assert!(config_path.is_file(), ".alongconfig was not created");
    let cfg = git2::Config::open(&config_path).unwrap();
    assert_eq!(
        cfg.get_string("along.storeUrl").unwrap(),
        "https://example.com/store.git"
    );
    assert!(!cfg.get_bool("along.modifyPermissions").unwrap());
    assert!(!cfg.get_bool("along.trackUncommitted").unwrap());
    assert_eq!(cfg.get_string("along.pullThreshold").unwrap(), "60");
}

#[test]
fn defaults_to_refs_store_when_url_is_omitted() {
    let dir = tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.remote("origin", "https://example.com/project.git")
        .unwrap();

    common::git_along_in(dir.path())
        .args(["setup"])
        .assert()
        .success();

    let cfg = git2::Config::open(&dir.path().join(".alongconfig")).unwrap();
    assert!(cfg.get_string("along.storeUrl").is_err());
}

#[test]
fn rejects_omitted_url_without_a_remote() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
        .args(["setup"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no remote"));

    assert!(!dir.path().join(".alongconfig").exists());
}

#[test]
fn rejects_unrecognized_store_url() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
        .args(["setup", "ftp://nope/somewhere"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid store URL"));

    // Failed setup should not have left a config file behind.
    assert!(!dir.path().join(".alongconfig").exists());
}

#[test]
fn rejects_setup_outside_git_repo() {
    let dir = tempdir().unwrap();
    // No git init.
    common::git_along_in(dir.path())
        .args(["setup", "https://example.com/store.git"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not in a git repository"));
}

#[test]
fn parses_store_headers_into_config() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
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

    let cfg = git2::Config::open(&dir.path().join(".alongconfig")).unwrap();
    assert_eq!(
        cfg.get_string("along.header.X-Access-Key").unwrap(),
        "secret"
    );
    assert_eq!(
        cfg.get_string("along.header.X-Bin-Versioning").unwrap(),
        "false"
    );
}

#[test]
fn malformed_store_header_is_rejected() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
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
fn update_hooks_installs_each_hook() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
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
            body.contains("git along update"),
            "hook {name} should call `git along update`"
        );
    }
}

#[test]
fn modify_permissions_disables_core_filemode() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    common::git_along_in(dir.path())
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
