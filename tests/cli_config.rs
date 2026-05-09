//! End-to-end tests for `gitalong config`.

mod common;

use std::fs;
use std::path::Path;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::tempdir;

const SAMPLE_CONFIG: &str = r#"{
  "store_url": "https://example.com/store.git",
  "store_headers": {},
  "modify_permissions": true,
  "track_binaries": false,
  "tracked_extensions": [".jpg", ".png"],
  "pull_threshold": 30.0,
  "track_uncommitted": true
}
"#;

/// Initialize a git repo at `path` and drop the sample config into it. Tests
/// that don't want a config call `git2::Repository::init` directly instead.
fn init_managed_repo(path: &Path) {
    git2::Repository::init(path).unwrap();
    fs::write(path.join(".gitalong.json"), SAMPLE_CONFIG).unwrap();
}

#[test]
fn outputs_value_for_known_property() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}

#[test]
fn renders_bool_as_lowercase() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["config", "modify-permissions"])
        .assert()
        .success()
        .stdout("true\n");

    common::gitalong_in(dir.path())
        .args(["config", "track-binaries"])
        .assert()
        .success()
        .stdout("false\n");
}

#[test]
fn unknown_property_prints_nothing() {
    let dir = tempdir().unwrap();
    init_managed_repo(dir.path());

    common::gitalong_in(dir.path())
        .args(["config", "no-such-key"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn no_config_file_exits_silently() {
    // Git repo exists but no .gitalong.json — like running on a repo that
    // hasn't been set up yet.
    let dir = tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();

    common::gitalong_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn non_git_path_exits_silently() {
    // Outside any git repository entirely.
    let dir = tempdir().unwrap();

    common::gitalong_in(dir.path())
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

    common::gitalong_in(&nested)
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}

#[test]
fn invalid_config_returns_error() {
    let dir = tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join(".gitalong.json"), b"{ broken").unwrap();

    common::gitalong_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid gitalong configuration"));
}

#[test]
fn explicit_repository_flag_overrides_cwd() {
    let cwd = tempdir().unwrap();
    let repo = tempdir().unwrap();
    init_managed_repo(repo.path());

    common::gitalong_in(cwd.path())
        .args(["-C", repo.path().to_str().unwrap(), "config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}
