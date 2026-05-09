//! The [`Repository`] aggregator.
//!
//! Wraps a `git2::Repository` together with the parsed gitalong [`Config`] and
//! exposes the contextual helpers (working directory, path translation,
//! identity context) the higher-level commands operate on.

use std::path::{Path, PathBuf};

use crate::config::{CONFIG_BASENAME, Config};
use crate::error::{Error, Result};

/// A git repository plus a loaded gitalong configuration.
///
/// Construct one with [`Repository::open`] (errors if not set up) or
/// [`Repository::discover`] (returns `None` when the path is outside any git
/// repo or the repo has no `.gitalong.json`).
pub struct Repository {
    inner: git2::Repository,
    working_dir: PathBuf,
    config: Config,
}

/// Identity context attached to commits issued by this clone.
///
/// Mirrors the Python `context_dict` of `host`, `user`, `clone`. The `clone`
/// is the canonicalized working-tree path so two clones at different aliased
/// paths are still distinguishable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub host: String,
    pub user: String,
    pub clone: PathBuf,
}

impl Repository {
    /// Open the gitalong repository containing `path`.
    ///
    /// Walks upward from `path` looking for a git repository, then loads
    /// `.gitalong.json` from its working tree. Returns
    /// [`Error::NotSetup`] when the repo exists but has no config, and any
    /// other git or I/O error otherwise.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let inner = git2::Repository::discover(path.as_ref())?;
        let working_dir = inner
            .workdir()
            .ok_or_else(|| Error::InvalidConfig("bare repositories are not supported".into()))?
            .to_path_buf();
        let config = Config::load(&working_dir.join(CONFIG_BASENAME))?;
        Ok(Self {
            inner,
            working_dir,
            config,
        })
    }

    /// Like [`Repository::open`] but returns `None` when the path is not in a
    /// git repo, or when the repo has no gitalong config. Mirrors the Python
    /// `Repository.from_filename` semantics for callers that want to silently
    /// skip non-managed paths.
    pub fn discover(path: impl AsRef<Path>) -> Result<Option<Self>> {
        match Self::open(path) {
            Ok(repo) => Ok(Some(repo)),
            Err(Error::NotSetup(_)) => Ok(None),
            Err(Error::Git(e))
                if matches!(
                    e.code(),
                    git2::ErrorCode::NotFound | git2::ErrorCode::Ambiguous
                ) =>
            {
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }

    /// Absolute path of the working tree (the directory holding `.gitalong.json`).
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// Path to `.gitalong.json` for this repository.
    pub fn config_path(&self) -> PathBuf {
        Config::path_for(&self.working_dir)
    }

    /// Read-only access to the parsed config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Underlying `git2::Repository`. Exposed for the operations that haven't
    /// been ported yet; callers should prefer the higher-level helpers when
    /// they exist.
    pub fn git(&self) -> &git2::Repository {
        &self.inner
    }

    /// Convert an absolute path inside the working tree to its repo-relative form.
    ///
    /// Handles macOS-style path aliasing (e.g. `/tmp` ↔ `/private/tmp`) by
    /// canonicalizing the input when a direct prefix match fails. Paths
    /// outside the working tree, and relative paths, are returned unchanged.
    pub fn relative_path(&self, filename: &Path) -> PathBuf {
        if !filename.is_absolute() {
            return filename.to_path_buf();
        }
        if let Ok(rel) = filename.strip_prefix(&self.working_dir) {
            return rel.to_path_buf();
        }
        if let Some(canon) = canonicalize_lenient(filename)
            && let Ok(rel) = canon.strip_prefix(&self.working_dir)
        {
            return rel.to_path_buf();
        }
        filename.to_path_buf()
    }

    /// Convert a repo-relative path to an absolute path inside the working tree.
    ///
    /// Already-absolute paths are returned unchanged.
    pub fn absolute_path(&self, filename: &Path) -> PathBuf {
        if filename.is_absolute() {
            filename.to_path_buf()
        } else {
            self.working_dir.join(filename)
        }
    }

    /// Identity context attached to commits this clone issues.
    pub fn context(&self) -> Context {
        Context {
            host: whoami::fallible::hostname().unwrap_or_else(|_| "unknown".into()),
            user: whoami::username(),
            clone: real_path(&self.working_dir),
        }
    }

    /// Resolve the directory git should look in for hooks. Honors
    /// `core.hooksPath` if set in the repository config, otherwise falls back
    /// to `<gitdir>/hooks`.
    pub fn hooks_path(&self) -> Result<PathBuf> {
        let cfg = self.inner.config()?;
        if let Ok(custom) = cfg.get_path("core.hooksPath") {
            return Ok(if custom.is_absolute() {
                custom
            } else {
                self.working_dir.join(custom)
            });
        }
        Ok(self.inner.path().join("hooks"))
    }

    /// Install the gitalong git hooks into [`hooks_path`](Self::hooks_path).
    pub fn install_hooks(&self) -> Result<()> {
        crate::hooks::install(&self.hooks_path()?)
    }

    /// Append the gitalong directives to `.gitignore` if they aren't already
    /// present. Idempotent: running twice leaves the file unchanged the
    /// second time.
    pub fn update_gitignore(&self) -> Result<()> {
        let path = self.working_dir.join(".gitignore");
        let existing = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        if existing.contains(crate::hooks::GITIGNORE_PATCH) {
            return Ok(());
        }
        let needs_separator = !existing.is_empty() && !existing.ends_with('\n');
        let mut next = existing;
        if needs_separator {
            next.push('\n');
        }
        next.push_str(crate::hooks::GITIGNORE_PATCH);
        std::fs::write(&path, next)?;
        Ok(())
    }

    /// Disable git's `core.fileMode` tracking so user-driven `chmod` calls
    /// (which gitalong uses to enforce claims when `modify_permissions` is on)
    /// don't show up as dirty diffs.
    pub fn disable_file_mode_tracking(&self) -> Result<()> {
        let mut cfg = self.inner.config()?;
        cfg.set_bool("core.fileMode", false)?;
        Ok(())
    }
}

/// Resolve symlinks in `p`, falling back to `p` itself when canonicalization
/// fails (e.g. on platforms where the path can't be probed).
fn real_path(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Try to canonicalize a path even if it doesn't exist yet by canonicalizing
/// the deepest existing ancestor and rejoining the remaining components.
///
/// Used by [`Repository::relative_path`] to bridge macOS path aliases (e.g.
/// `/tmp/foo/new.txt` against a canonical working tree at `/private/tmp/foo`).
fn canonicalize_lenient(p: &Path) -> Option<PathBuf> {
    if let Ok(c) = std::fs::canonicalize(p) {
        return Some(c);
    }
    let mut suffix: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor: &Path = p;
    while let Some(parent) = cursor.parent() {
        if let Some(name) = cursor.file_name() {
            suffix.push(name);
        }
        if let Ok(canon) = std::fs::canonicalize(parent) {
            let mut out = canon;
            for name in suffix.iter().rev() {
                out.push(name);
            }
            return Some(out);
        }
        cursor = parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Initialize a fresh git repo with a `.gitalong.json` so `Repository::open`
    /// can succeed.
    fn fixture(store_url: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let cfg = Config {
            store_url: store_url.into(),
            ..Config::default()
        };
        cfg.save(&dir.path().join(CONFIG_BASENAME)).unwrap();
        dir
    }

    #[test]
    fn open_loads_config_from_working_tree() {
        let dir = fixture("https://example.com/store.git");
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.config().store_url, "https://example.com/store.git");
        // canonicalize() may strip /private prefixes etc; compare canonicalized.
        let want = std::fs::canonicalize(dir.path()).unwrap();
        let got = std::fs::canonicalize(repo.working_dir()).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn open_walks_upward() {
        let dir = fixture("x.git");
        let nested = dir.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let repo = Repository::open(&nested).unwrap();
        assert_eq!(repo.config().store_url, "x.git");
    }

    #[test]
    fn open_without_config_returns_not_setup() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        match Repository::open(dir.path()) {
            Err(Error::NotSetup(_)) => {}
            Err(other) => panic!("expected NotSetup, got {other:?}"),
            Ok(_) => panic!("expected NotSetup, got Ok"),
        }
    }

    #[test]
    fn discover_returns_none_for_non_git_path() {
        let dir = tempdir().unwrap();
        // No git init.
        assert!(Repository::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn discover_returns_none_when_not_setup() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        assert!(Repository::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn relative_path_strips_working_dir() {
        let dir = fixture("x.git");
        let repo = Repository::open(dir.path()).unwrap();
        let abs = dir.path().join("subdir/file.txt");
        let rel = repo.relative_path(&abs);
        assert_eq!(rel, Path::new("subdir/file.txt"));
    }

    #[test]
    fn relative_path_passes_through_relative_input() {
        let dir = fixture("x.git");
        let repo = Repository::open(dir.path()).unwrap();
        let rel = repo.relative_path(Path::new("already/relative.txt"));
        assert_eq!(rel, Path::new("already/relative.txt"));
    }

    #[test]
    fn absolute_path_joins_relative_with_working_dir() {
        let dir = fixture("x.git");
        let repo = Repository::open(dir.path()).unwrap();
        let abs = repo.absolute_path(Path::new("foo.txt"));
        assert_eq!(abs, repo.working_dir().join("foo.txt"));
    }

    #[test]
    fn absolute_path_passes_through_absolute_input() {
        let dir = fixture("x.git");
        let repo = Repository::open(dir.path()).unwrap();
        let already = Path::new("/absolute/foo.txt");
        assert_eq!(repo.absolute_path(already), already);
    }

    #[test]
    fn context_has_non_empty_user_and_host() {
        let dir = fixture("x.git");
        let repo = Repository::open(dir.path()).unwrap();
        let ctx = repo.context();
        assert!(!ctx.user.is_empty());
        assert!(!ctx.host.is_empty());
        assert!(ctx.clone.is_absolute());
    }
}
