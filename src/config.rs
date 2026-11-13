//! `.alongconfig` parsing, serialization, and property lookup.
//!
//! The file uses git config syntax, like `.lfsconfig`, with every key under
//! the `along` section. The same keys in the repository's git config override
//! it, so one clone can differ from the team without touching the shared file.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Filename used for the Git Along config in the root of the managed repository.
pub const CONFIG_BASENAME: &str = ".alongconfig";

const STORE_URL: &str = "along.storeUrl";
const MODIFY_PERMISSIONS: &str = "along.modifyPermissions";
const TRACK_BINARIES: &str = "along.trackBinaries";
const TRACKED_EXTENSIONS: &str = "along.trackedExtensions";
const PULL_THRESHOLD: &str = "along.pullThreshold";
const TRACK_UNCOMMITTED: &str = "along.trackUncommitted";

/// Subsection holding one key per HTTP header, as in `[along "header"]`.
const HEADER_PREFIX: &str = "along.header.";

/// Parsed representation of `.alongconfig`.
///
/// Keys missing from the file keep their defaults, so a partial file is valid.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// URL or local path to the store. Empty selects the refs store on the
    /// repository's own remote, a `.git` suffix a git store, and a
    /// `https://api.jsonbin.io` prefix a JSONBin store.
    pub store_url: String,

    /// HTTP headers attached to store requests. Values starting with `$` are
    /// expanded against the process environment at request time. Git config
    /// lowercases key names, so header names come back lowercased.
    pub store_headers: BTreeMap<String, String>,

    /// When true, Git Along manages file write permissions to enforce claims.
    pub modify_permissions: bool,

    /// When true, all auto-detected binary files are tracked.
    pub track_binaries: bool,

    /// File extensions to track (each entry includes the leading dot, e.g. `.jpg`).
    pub tracked_extensions: Vec<String>,

    /// Cache window for store pulls in seconds.
    pub pull_threshold: f64,

    /// When true, uncommitted changes are stored as sha-less commits.
    pub track_uncommitted: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            store_url: String::new(),
            store_headers: BTreeMap::new(),
            modify_permissions: false,
            track_binaries: false,
            tracked_extensions: Vec::new(),
            pull_threshold: 60.0,
            track_uncommitted: false,
        }
    }
}

impl Config {
    /// Read `.alongconfig` from `path`, then apply the `along` keys of
    /// `overrides` on top of it.
    ///
    /// Returns [`Error::NotSetup`] if the file does not exist and
    /// [`Error::InvalidConfig`] if it or a value in it cannot be parsed.
    pub fn load(path: &Path, overrides: Option<&git2::Config>) -> Result<Self> {
        if !path.is_file() {
            return Err(Error::NotSetup(path.to_path_buf()));
        }
        let file = git2::Config::open(path).map_err(|e| {
            Error::InvalidConfig(format!("could not parse {path:?}: {}", e.message()))
        })?;
        let mut config = Self::default();
        config.apply(&file)?;
        if let Some(overrides) = overrides {
            config.apply(overrides)?;
        }
        Ok(config)
    }

    /// Write the config to `path` in git config syntax, replacing the file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut out = String::from("[along]\n");
        if !self.store_url.is_empty() {
            line(&mut out, STORE_URL, &self.store_url);
        }
        line(
            &mut out,
            MODIFY_PERMISSIONS,
            bool_str(self.modify_permissions),
        );
        line(&mut out, TRACK_BINARIES, bool_str(self.track_binaries));
        for extension in &self.tracked_extensions {
            line(&mut out, TRACKED_EXTENSIONS, extension);
        }
        line(&mut out, PULL_THRESHOLD, &self.pull_threshold.to_string());
        line(
            &mut out,
            TRACK_UNCOMMITTED,
            bool_str(self.track_uncommitted),
        );
        if !self.store_headers.is_empty() {
            out.push_str("[along \"header\"]\n");
            for (name, value) in &self.store_headers {
                if !is_key_name(name) {
                    return Err(Error::InvalidConfig(format!(
                        "header `{name}` can only use letters, digits and hyphens"
                    )));
                }
                let _ = writeln!(out, "\t{name} = {}", quote(value));
            }
        }
        std::fs::write(path, out)?;
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
            "modify_permissions" => bool_str(self.modify_permissions).into(),
            "track_binaries" => bool_str(self.track_binaries).into(),
            "tracked_extensions" => serde_json::to_string(&self.tracked_extensions).ok()?,
            "pull_threshold" => self.pull_threshold.to_string(),
            "track_uncommitted" => bool_str(self.track_uncommitted).into(),
            _ => return None,
        })
    }

    /// Resolve the canonical config path for a given repository working tree.
    pub fn path_for(working_dir: &Path) -> PathBuf {
        working_dir.join(CONFIG_BASENAME)
    }

    /// Overwrite every field `cfg` sets.
    fn apply(&mut self, cfg: &git2::Config) -> Result<()> {
        if let Some(value) = string(cfg, STORE_URL)? {
            self.store_url = value;
        }
        if let Some(value) = boolean(cfg, MODIFY_PERMISSIONS)? {
            self.modify_permissions = value;
        }
        if let Some(value) = boolean(cfg, TRACK_BINARIES)? {
            self.track_binaries = value;
        }
        let extensions = values(cfg, TRACKED_EXTENSIONS)?;
        if !extensions.is_empty() {
            self.tracked_extensions = extensions;
        }
        if let Some(value) = string(cfg, PULL_THRESHOLD)? {
            self.pull_threshold = value.parse().map_err(|_| {
                Error::InvalidConfig(format!("{PULL_THRESHOLD} is not a number: {value}"))
            })?;
        }
        if let Some(value) = boolean(cfg, TRACK_UNCOMMITTED)? {
            self.track_uncommitted = value;
        }
        let mut headers = cfg.entries(Some(r"^along\.header\."))?;
        while let Some(entry) = headers.next() {
            let entry = entry?;
            if let (Some(name), Some(value)) = (entry.name(), entry.value()) {
                let name = name.trim_start_matches(HEADER_PREFIX);
                self.store_headers
                    .insert(name.to_string(), value.to_string());
            }
        }
        Ok(())
    }
}

fn string(cfg: &git2::Config, key: &str) -> Result<Option<String>> {
    match cfg.get_string(key) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(Error::InvalidConfig(format!("{key}: {}", e.message()))),
    }
}

fn boolean(cfg: &git2::Config, key: &str) -> Result<Option<bool>> {
    match cfg.get_bool(key) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(Error::InvalidConfig(format!("{key}: {}", e.message()))),
    }
}

/// Every value of a multivar, in file order.
fn values(cfg: &git2::Config, key: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut entries = match cfg.multivar(key, None) {
        Ok(entries) => entries,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next() {
        if let Some(value) = entry?.value() {
            out.push(value.to_string());
        }
    }
    Ok(out)
}

/// Append `key = value` under the `[along]` section opened by the caller.
fn line(out: &mut String, key: &str, value: &str) {
    let name = key.trim_start_matches("along.");
    let _ = writeln!(out, "\t{name} = {}", quote(value));
}

/// Quote a value only when git config would otherwise misread it.
fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && !value.starts_with(char::is_whitespace)
        && !value.ends_with(char::is_whitespace)
        && !value.contains(['#', ';', '"', '\\', '\n']);
    if plain {
        return value.to_string();
    }
    let escaped = value
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', r"\n");
    format!("\"{escaped}\"")
}

/// Whether `name` is a valid git config variable name.
fn is_key_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn load_text(text: &str) -> Result<Config> {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        std::fs::write(&path, text).unwrap();
        Config::load(&path, None)
    }

    #[test]
    fn defaults() {
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
    fn missing_keys_use_defaults() {
        let cfg = load_text("[along]\n\tstoreUrl = x.git\n").unwrap();
        assert_eq!(cfg.store_url, "x.git");
        assert_eq!(cfg.pull_threshold, 60.0);
        assert!(!cfg.track_binaries);
    }

    #[test]
    fn reads_comments_and_any_key_case() {
        let cfg = load_text(
            "# Shared with the team\n[along]\n\ttrackbinaries = yes ; Unreal assets\n\tTRACKEDEXTENSIONS = .uasset\n",
        )
        .unwrap();
        assert!(cfg.track_binaries);
        assert_eq!(cfg.tracked_extensions, [".uasset"]);
    }

    #[test]
    fn round_trip_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);

        let original = Config {
            store_url: "https://api.jsonbin.io/v3/b/foo#main".into(),
            store_headers: BTreeMap::from([
                ("x-access-key".into(), "$JSONBIN_KEY".into()),
                ("x-bin-versioning".into(), "false".into()),
            ]),
            modify_permissions: true,
            track_binaries: false,
            tracked_extensions: vec![".jpg".into(), ".png".into()],
            pull_threshold: 30.5,
            track_uncommitted: true,
        };
        original.save(&path).unwrap();
        assert_eq!(Config::load(&path, None).unwrap(), original);
    }

    #[test]
    fn saved_file_reads_like_a_git_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        Config {
            tracked_extensions: vec![".uasset".into()],
            store_headers: BTreeMap::from([("X-Access-Key".into(), "$KEY".into())]),
            ..Config::default()
        }
        .save(&path)
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[along]\n\tmodifyPermissions = false\n\ttrackBinaries = false\n\ttrackedExtensions = .uasset\n\tpullThreshold = 60\n\ttrackUncommitted = false\n[along \"header\"]\n\tX-Access-Key = $KEY\n"
        );
    }

    #[test]
    fn save_rejects_header_names_git_config_cannot_hold() {
        let dir = tempdir().unwrap();
        let cfg = Config {
            store_headers: BTreeMap::from([("X_Key".into(), "v".into())]),
            ..Config::default()
        };
        assert!(matches!(
            cfg.save(&dir.path().join(CONFIG_BASENAME)),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn overrides_win_over_the_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        std::fs::write(
            &path,
            "[along]\n\tpullThreshold = 60\n\ttrackedExtensions = .jpg\n",
        )
        .unwrap();
        let local = dir.path().join("local");
        std::fs::write(
            &local,
            "[along]\n\tpullThreshold = 5\n\ttrackedExtensions = .png\n\ttrackedExtensions = .tga\n",
        )
        .unwrap();
        let overrides = git2::Config::open(&local).unwrap();
        let cfg = Config::load(&path, Some(&overrides)).unwrap();
        assert_eq!(cfg.pull_threshold, 5.0);
        assert_eq!(cfg.tracked_extensions, [".png", ".tga"]);
    }

    #[test]
    fn load_missing_file_returns_not_setup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(CONFIG_BASENAME);
        match Config::load(&path, None) {
            Err(Error::NotSetup(p)) => assert_eq!(p, path),
            other => panic!("expected NotSetup, got {other:?}"),
        }
    }

    #[test]
    fn load_garbage_returns_invalid_config() {
        assert!(matches!(
            load_text("[along\n\tbroken"),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(
            load_text("[along]\n\ttrackBinaries = maybe\n"),
            Err(Error::InvalidConfig(_))
        ));
        assert!(matches!(
            load_text("[along]\n\tpullThreshold = soon\n"),
            Err(Error::InvalidConfig(_))
        ));
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
        assert_eq!(path, Path::new("/tmp/repo/.alongconfig"));
    }
}
