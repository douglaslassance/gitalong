//! `.gitalong.json` parsing, serialization, and property lookup.
//!
//! The schema mirrors the Python 0.x contract: the same field names with the
//! same defaults, so a config file written by one implementation can be read
//! by the other. The on-disk form is pretty-printed JSON with sorted keys.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Filename used for the gitalong config in the root of the managed repository.
pub const CONFIG_BASENAME: &str = ".gitalong.json";

/// Parsed representation of `.gitalong.json`.
///
/// Fields default to values that match the Python implementation so that an
/// empty or partial config file behaves identically across versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// URL or local path to the store. A `.git` suffix selects a git store; a
    /// `https://api.jsonbin.io` prefix selects a JSONBin store.
    #[serde(default)]
    pub store_url: String,

    /// HTTP headers attached to store requests. Values starting with `$` are
    /// expanded against the process environment at request time.
    #[serde(default)]
    pub store_headers: BTreeMap<String, String>,

    /// When true, gitalong manages file write permissions to enforce claims.
    #[serde(default)]
    pub modify_permissions: bool,

    /// When true, all auto-detected binary files are tracked.
    #[serde(default)]
    pub track_binaries: bool,

    /// File extensions to track (each entry includes the leading dot, e.g. `.jpg`).
    #[serde(default)]
    pub tracked_extensions: Vec<String>,

    /// Cache window for store pulls in seconds.
    #[serde(default = "default_pull_threshold")]
    pub pull_threshold: f64,

    /// When true, uncommitted changes are stored as sha-less commits.
    #[serde(default)]
    pub track_uncommitted: bool,
}

fn default_pull_threshold() -> f64 {
    60.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            store_url: String::new(),
            store_headers: BTreeMap::new(),
            modify_permissions: false,
            track_binaries: false,
            tracked_extensions: Vec::new(),
            pull_threshold: default_pull_threshold(),
            track_uncommitted: false,
        }
    }
}

impl Config {
    /// Read and parse a `.gitalong.json` from disk.
    ///
    /// Returns [`Error::NotSetup`] if the file does not exist and
    /// [`Error::InvalidConfig`] if it exists but cannot be parsed.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NotSetup(path.to_path_buf()));
            }
            Err(e) => return Err(Error::Io(e)),
        };
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidConfig(format!("could not parse {path:?}: {e}")))?;
        Ok(config)
    }

    /// Write the config to disk with sorted keys and four-space indentation.
    pub fn save(&self, path: &Path) -> Result<()> {
        // serde_json with our BTreeMap field already produces sorted keys at
        // every nesting level. Top-level field order follows the struct layout
        // and is stable.
        let pretty = serde_json::to_string_pretty(self)?;
        fs::write(path, pretty.as_bytes())?;
        Ok(())
    }

    /// Look up a single property by its dotted/dashed name.
    ///
    /// Hyphens are translated to underscores so callers can pass the friendly
    /// CLI form (`store-url`, `track-binaries`). Returns `None` for unknown
    /// properties; bool values render as `true`/`false`, scalars as their
    /// natural string form, and collections as JSON.
    pub fn property(&self, name: &str) -> Option<String> {
        let key = name.replace('-', "_");
        Some(match key.as_str() {
            "store_url" => self.store_url.clone(),
            "store_headers" => serde_json::to_string(&self.store_headers).ok()?,
            "modify_permissions" => bool_str(self.modify_permissions),
            "track_binaries" => bool_str(self.track_binaries),
            "tracked_extensions" => serde_json::to_string(&self.tracked_extensions).ok()?,
            "pull_threshold" => self.pull_threshold.to_string(),
            "track_uncommitted" => bool_str(self.track_uncommitted),
            _ => return None,
        })
    }

    /// Resolve the canonical config path for a given repository working tree.
    pub fn path_for(working_dir: &Path) -> PathBuf {
        working_dir.join(CONFIG_BASENAME)
    }
}

fn bool_str(b: bool) -> String {
    if b { "true".into() } else { "false".into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_match_python() {
        let cfg = Config::default();
        assert_eq!(cfg.store_url, "");
        assert!(cfg.store_headers.is_empty());
        assert!(!cfg.modify_permissions);
        assert!(!cfg.track_binaries);
        assert!(cfg.tracked_extensions.is_empty());
        assert_eq!(cfg.pull_threshold, 60.0);
        assert!(!cfg.track_uncommitted);
    }

    #[test]
    fn missing_fields_use_defaults() {
        // Only store_url is provided; the rest must fall back to defaults.
        let cfg: Config = serde_json::from_str(r#"{"store_url": "x.git"}"#).unwrap();
        assert_eq!(cfg.store_url, "x.git");
        assert_eq!(cfg.pull_threshold, 60.0);
        assert!(!cfg.track_binaries);
    }

    #[test]
    fn round_trip_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);

        let original = Config {
            store_url: "https://api.jsonbin.io/v3/b/foo".into(),
            store_headers: BTreeMap::from([
                ("X-Access-Key".into(), "$JSONBIN_KEY".into()),
                ("X-Bin-Versioning".into(), "false".into()),
            ]),
            modify_permissions: true,
            track_binaries: false,
            tracked_extensions: vec![".jpg".into(), ".png".into()],
            pull_threshold: 30.0,
            track_uncommitted: true,
        };
        original.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn load_missing_file_returns_not_setup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        match Config::load(&path) {
            Err(Error::NotSetup(p)) => assert_eq!(p, path),
            other => panic!("expected NotSetup, got {other:?}"),
        }
    }

    #[test]
    fn load_garbage_returns_invalid_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        std::fs::write(&path, b"{ not valid json").unwrap();
        match Config::load(&path) {
            Err(Error::InvalidConfig(_)) => {}
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn property_lookup_translates_hyphens() {
        let cfg = Config {
            store_url: "x.git".into(),
            modify_permissions: true,
            track_binaries: false,
            pull_threshold: 42.5,
            tracked_extensions: vec![".jpg".into()],
            ..Config::default()
        };
        assert_eq!(cfg.property("store-url").as_deref(), Some("x.git"));
        assert_eq!(cfg.property("modify-permissions").as_deref(), Some("true"));
        assert_eq!(cfg.property("track-binaries").as_deref(), Some("false"));
        assert_eq!(cfg.property("pull-threshold").as_deref(), Some("42.5"));
        assert_eq!(
            cfg.property("tracked-extensions").as_deref(),
            Some(r#"[".jpg"]"#)
        );
        assert_eq!(cfg.property("does-not-exist"), None);
    }

    #[test]
    fn config_path_for_joins_basename() {
        let path = Config::path_for(Path::new("/tmp/repo"));
        assert_eq!(path, Path::new("/tmp/repo/.gitalong.json"));
    }
}
