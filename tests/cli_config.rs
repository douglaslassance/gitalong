//! End-to-end tests for `git along config`.

mod common;

use std::fs;
use std::path::Path;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

const SAMPLE_CONFIG: &str = "[along]
	storeUrl = https://example.com/store.git
	modifyPermissions = true
	trackBinaries = false
	trackedExtensions = .jpg
	trackedExtensions = .png
	pullThreshold = 30
	trackUncommitted = true
";

/// Initialize a git repo at `path` and drop the sample config into it. Tests
/// that don't want a config call `git2::Repository::init` directly instead.
fn init_managed_repo(path: &Path) {
    git2::Repository::init(path).unwrap();
    fs::write(path.join(".alongconfig"), SAMPLE_CONFIG).unwrap();
}

#[test]
fn outputs_value_for_known_property() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::git_along_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}

#[test]
fn git_config_overrides_the_shared_file() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());
    let repo = git2::Repository::open(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_str("along.pullThreshold", "5")
        .unwrap();

    common::git_along_in(dir.path())
        .args(["config", "pull-threshold"])
        .assert()
        .success()
        .stdout("5\n");
}

#[test]
fn renders_bool_as_lowercase() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::git_along_in(dir.path())
        .args(["config", "modify-permissions"])
        .assert()
        .success()
        .stdout("true\n");

    common::git_along_in(dir.path())
        .args(["config", "track-binaries"])
        .assert()
        .success()
        .stdout("false\n");
}

#[test]
fn unknown_property_prints_nothing() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::git_along_in(dir.path())
        .args(["config", "no-such-key"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn no_config_file_exits_silently() {
    // Git repo exists but no .alongconfig — like running on a repo that
    // hasn't been set up yet.
    let dir = tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();

    common::git_along_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn non_git_path_exits_silently() {
    // Outside any git repository entirely.
    let dir = tempdir().unwrap();

    common::git_along_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn finds_config_in_parent_directory() {
    let root = tempdir().unwrap();
    init_managed_repo(root.path());

    let nested = root.path().join("a").join("b");
    fs::create_dir_all(&nested).unwrap();

    common::git_along_in(&nested)
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}

#[test]
fn invalid_config_returns_error() {
    let dir = tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join(".alongconfig"), b"[along\n\tbroken").unwrap();

    common::git_along_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid Git Along configuration"));
}

#[test]
fn explicit_repository_flag_overrides_cwd() {
    let cwd = tempdir().unwrap();
    let repo = tempdir().unwrap();
    init_managed_repo(repo.path());

    common::git_along_in(cwd.path())
        .args(["-C", repo.path().to_str().unwrap(), "config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}
