//! End-to-end tests for `gitalong config`.

mod common;

use std::fs;

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

#[test]
fn outputs_value_for_known_property() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitalong.json"), SAMPLE_CONFIG).unwrap();

    common::gitalong_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}

#[test]
fn renders_bool_as_lowercase() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitalong.json"), SAMPLE_CONFIG).unwrap();

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
    fs::write(dir.path().join(".gitalong.json"), SAMPLE_CONFIG).unwrap();

    common::gitalong_in(dir.path())
        .args(["config", "no-such-key"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn no_config_file_exits_silently() {
    let dir = tempdir().unwrap();
    // No .gitalong.json on disk.
    common::gitalong_in(dir.path())
        .args(["config", "store-url"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn finds_config_in_parent_directory() {
    let root = tempdir().unwrap();
    fs::write(root.path().join(".gitalong.json"), SAMPLE_CONFIG).unwrap();

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
    fs::write(repo.path().join(".gitalong.json"), SAMPLE_CONFIG).unwrap();

    common::gitalong_in(cwd.path())
        .args(["-C", repo.path().to_str().unwrap(), "config", "store-url"])
        .assert()
        .success()
        .stdout("https://example.com/store.git\n");
}
