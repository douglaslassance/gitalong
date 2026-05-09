//! Git-repository-backed [`Store`](super::Store).
//!
//! Clones the configured store URL into `<managed_repo>/.gitalong/`, then
//! reads and writes `commits.json` as a regular file in that working tree.
//! Network operations (`clone`, `pull`, `push`) shell out to `git` so they
//! pick up the user's existing credential helpers and SSH agent.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use crate::commit::Commit;
use crate::error::{Error, Result};
use crate::repository::Repository;

/// Filename of the JSON document inside the store repository.
const COMMITS_FILENAME: &str = "commits.json";

/// Subdirectory of the managed repository where the store is cloned.
const CLONE_DIRNAME: &str = ".gitalong";

/// `Store` backed by a cloned git repository.
pub struct GitStore {
    /// Working tree of the cloned store repo.
    clone_path: PathBuf,
    /// Cache window in seconds for `git pull` calls.
    pull_threshold: f64,
}

impl GitStore {
    /// Open the clone at `<managed>/.gitalong/`, cloning the configured store
    /// URL on first use.
    pub fn open_or_clone(repo: &Repository) -> Result<Self> {
        let clone_path = repo.working_dir().join(CLONE_DIRNAME);
        let store_url = &repo.config().store_url;

        if !clone_path.join(".git").is_dir() {
            run_git(&[
                "clone",
                store_url,
                clone_path
                    .to_str()
                    .ok_or_else(|| Error::InvalidConfig("non-UTF8 path".into()))?,
            ])
            .map_err(|e| {
                Error::StoreUnreachable(format!("failed to clone store from {store_url}: {e}"))
            })?;
        }

        Ok(Self {
            clone_path,
            pull_threshold: repo.config().pull_threshold,
        })
    }

    /// Path to `commits.json` inside the cloned store.
    fn commits_path(&self) -> PathBuf {
        self.clone_path.join(COMMITS_FILENAME)
    }

    /// Pull the store (subject to the cache window) and return all commits.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        if !self.pulled_within_threshold() {
            // Best-effort: a failed pull (offline, force-pushed remote, etc.)
            // shouldn't kill the read — we fall through to the cached file.
            let _ = run_git_in(
                &self.clone_path,
                &[
                    "pull",
                    "--ff",
                    "--rebase",
                    "--autostash",
                    "--quiet",
                    "--no-verify",
                ],
            );
        }
        self.read_local()
    }

    /// Persist `commits`. Writes the file, commits the change to the store
    /// repo, and pushes. Skips committing when the file content is unchanged.
    pub fn write(&mut self, commits: &[Commit]) -> Result<()> {
        let path = self.commits_path();
        let new_body = serde_json::to_string_pretty(commits)?;

        if let Ok(existing) = std::fs::read_to_string(&path)
            && existing == new_body
        {
            return Ok(());
        }

        std::fs::write(&path, new_body.as_bytes())?;
        run_git_in(&self.clone_path, &["add", COMMITS_FILENAME])?;

        // Configure a stable identity for store commits if the user hasn't
        // set one globally — without this, `git commit` errors out in
        // pristine environments (CI, tempdirs in tests).
        ensure_commit_identity(&self.clone_path)?;

        run_git_in(
            &self.clone_path,
            &["commit", "--quiet", "-m", "Update commits.json"],
        )?;
        run_git_in(&self.clone_path, &["push", "--quiet"])?;
        Ok(())
    }

    /// Read the local cache without touching the network.
    fn read_local(&self) -> Result<Vec<Commit>> {
        let path = self.commits_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        let commits: Vec<Commit> = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidConfig(format!("malformed {}: {e}", path.display())))?;
        Ok(commits)
    }

    /// `true` when the last fetch happened within the configured cache window.
    fn pulled_within_threshold(&self) -> bool {
        let fetch_head = self.clone_path.join(".git/FETCH_HEAD");
        let modified = match std::fs::metadata(&fetch_head).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let elapsed = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);
        elapsed.as_secs_f64() < self.pull_threshold
    }
}

/// Spawn `git <args>` and surface a useful error on non-zero exit.
fn run_git(args: &[&str]) -> Result<Output> {
    let output = Command::new("git").args(args).output()?;
    check_status(&output, args)?;
    Ok(output)
}

/// Spawn `git -C <dir> <args>`.
fn run_git_in(dir: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git").current_dir(dir).args(args).output()?;
    check_status(&output, args)?;
    Ok(output)
}

fn check_status(output: &Output, args: &[&str]) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let cmd = args.join(" ");
    Err(Error::StoreUnreachable(format!(
        "git {cmd} failed: {stderr}"
    )))
}

/// Set a fallback `user.name` / `user.email` on the store repo when the
/// environment hasn't supplied one. Only writes the local config; the user's
/// global identity is preserved.
fn ensure_commit_identity(dir: &Path) -> Result<()> {
    let repo = git2::Repository::open(dir)?;
    let mut cfg = repo.config()?;
    let snapshot = cfg.snapshot()?;
    if snapshot.get_string("user.name").is_err() {
        cfg.set_str("user.name", "gitalong")?;
    }
    if snapshot.get_string("user.email").is_err() {
        cfg.set_str("user.email", "gitalong@localhost")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use std::fs;
    use tempfile::TempDir;

    /// Stand up a complete fixture: a bare "store" repo, a managed repo
    /// configured against it, plus the [`Repository`] handle. Tests invoke
    /// `GitStore::open_or_clone` against the returned managed repo.
    struct Fixture {
        _store: TempDir,
        _managed: TempDir,
        repo: Repository,
    }

    fn make_fixture() -> Fixture {
        let store = tempfile::tempdir().unwrap();
        // Bare repo so we can push to it without setup gymnastics.
        Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(store.path())
            .output()
            .unwrap();

        let managed = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(managed.path())
            .output()
            .unwrap();

        let cfg = Config {
            store_url: format!("file://{}", store.path().display()),
            pull_threshold: 0.0, // always pull, so tests see remote updates
            ..Config::default()
        };
        cfg.save(&managed.path().join(CONFIG_BASENAME)).unwrap();

        let repo = Repository::open(managed.path()).unwrap();
        Fixture {
            _store: store,
            _managed: managed,
            repo,
        }
    }

    #[test]
    fn open_or_clone_creates_dot_gitalong_dir() {
        let f = make_fixture();
        let store = GitStore::open_or_clone(&f.repo).unwrap();
        assert!(store.clone_path.join(".git").is_dir());
    }

    #[test]
    fn read_on_empty_store_returns_empty_vec() {
        let f = make_fixture();
        let mut store = GitStore::open_or_clone(&f.repo).unwrap();
        assert!(store.read().unwrap().is_empty());
    }

    #[test]
    fn write_then_read_round_trips() {
        let f = make_fixture();
        let mut store = GitStore::open_or_clone(&f.repo).unwrap();

        let commits = vec![
            Commit {
                sha: Some("abc".into()),
                author: Some("Alice".into()),
                changes: vec!["a.txt".into()],
                ..Commit::default()
            },
            Commit {
                user: Some("alice".into()),
                host: Some("host-1".into()),
                clone: Some("/work/repo".into()),
                changes: vec!["draft.txt".into()],
                ..Commit::default()
            },
        ];

        store.write(&commits).unwrap();
        let back = store.read().unwrap();
        assert_eq!(back, commits);
    }

    #[test]
    fn write_is_skipped_when_unchanged() {
        let f = make_fixture();
        let mut store = GitStore::open_or_clone(&f.repo).unwrap();
        let commits = vec![Commit {
            sha: Some("abc".into()),
            ..Commit::default()
        }];

        store.write(&commits).unwrap();
        // Capture the HEAD sha after the first write.
        let head_after_first = head_sha(&store.clone_path);

        // Re-write identical content; HEAD must not advance.
        store.write(&commits).unwrap();
        let head_after_second = head_sha(&store.clone_path);
        assert_eq!(head_after_first, head_after_second);
    }

    #[test]
    fn second_clone_sees_first_clones_writes() {
        // Two managed repos sharing the same store URL — the second one's
        // GitStore should see what the first wrote, simulating two team
        // members running gitalong against the same store.
        let store = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(store.path())
            .output()
            .unwrap();
        let store_url = format!("file://{}", store.path().display());

        let alice = tempfile::tempdir().unwrap();
        let bob = tempfile::tempdir().unwrap();
        for managed in [alice.path(), bob.path()] {
            Command::new("git")
                .args(["init", "--initial-branch=main"])
                .arg(managed)
                .output()
                .unwrap();
            let cfg = Config {
                store_url: store_url.clone(),
                pull_threshold: 0.0,
                ..Config::default()
            };
            cfg.save(&managed.join(CONFIG_BASENAME)).unwrap();
        }

        let alice_repo = Repository::open(alice.path()).unwrap();
        let mut alice_store = GitStore::open_or_clone(&alice_repo).unwrap();
        alice_store
            .write(&[Commit {
                sha: Some("from-alice".into()),
                ..Commit::default()
            }])
            .unwrap();

        let bob_repo = Repository::open(bob.path()).unwrap();
        let mut bob_store = GitStore::open_or_clone(&bob_repo).unwrap();
        let commits = bob_store.read().unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha.as_deref(), Some("from-alice"));
    }

    fn head_sha(repo: &Path) -> String {
        let out = Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().into()
    }

    /// `git pull` on a bare-repo clone with no commits yet errors out in some
    /// git versions; the read path must tolerate that.
    #[test]
    fn pull_failure_does_not_break_read() {
        let f = make_fixture();
        let mut store = GitStore::open_or_clone(&f.repo).unwrap();
        // Drop the FETCH_HEAD so `pulled_within_threshold` returns false and
        // we'll try to pull again.
        let fetch_head = store.clone_path.join(".git/FETCH_HEAD");
        if fetch_head.exists() {
            fs::remove_file(&fetch_head).unwrap();
        }
        // Read should still succeed even if the bare remote has nothing yet.
        assert!(store.read().unwrap().is_empty());
    }
}
