//! Remote-ref-backed [`Store`](super::Store).
//!
//! Each clone publishes its records as one commit under
//! `refs/gitalong/v1/<clone-id>` on the managed repository's own remote. The
//! commit points at the empty tree and carries the JSON records in its
//! message, so nothing but metadata leaves the clone. Reads fetch the
//! namespace with the user's existing credentials. Writes force-push the
//! clone's own ref, which no other clone touches, so there is nothing to
//! merge and no write is rejected because of someone else's.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::commit::Commit;
use crate::error::{Error, Result};
use crate::repository::{Context, Repository};

use super::{run_git_in, touch};

/// Ref namespace shared by every clone. The version segment keeps a future
/// payload change from mixing with these records.
pub const NAMESPACE: &str = "refs/gitalong/v1";

/// Directory inside the git dir holding the clone id and the fetch marker.
const STATE_DIRNAME: &str = "gitalong";

/// `Store` backed by per-clone refs on the managed repository's remote.
pub struct RefStore {
    /// Working tree the `git` subprocesses run in.
    working_dir: PathBuf,
    /// Per-clone state directory inside the git dir.
    state_dir: PathBuf,
    remote: String,
    clone_id: String,
    /// Cache window in seconds for `git fetch` calls.
    pull_threshold: f64,
}

impl RefStore {
    /// Build the store for `repo`, minting a clone id on first use.
    pub fn new(repo: &Repository) -> Result<Self> {
        let remote = repo.remote_name()?.ok_or_else(|| {
            Error::InvalidConfig("the refs store needs a remote to publish to".into())
        })?;
        let state_dir = repo.git().path().join(STATE_DIRNAME);
        std::fs::create_dir_all(&state_dir)?;
        let clone_id = load_or_mint_clone_id(&state_dir.join("clone-id"), &repo.context())?;
        Ok(Self {
            working_dir: repo.working_dir().to_path_buf(),
            state_dir,
            remote,
            clone_id,
            pull_threshold: repo.config().pull_threshold,
        })
    }

    /// The ref this clone publishes to.
    pub fn own_ref(&self) -> String {
        format!("{NAMESPACE}/{}", self.clone_id)
    }

    /// Fetch the namespace (subject to the cache window) and return every
    /// clone's records. An unreachable remote falls back to the last fetch.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        self.fetch_if_stale()?;
        self.read_local()
    }

    /// Publish `ours` as this clone's records and return the complete view.
    /// The push is skipped when the records match the last publish.
    pub fn publish(&mut self, ours: &[Commit]) -> Result<Vec<Commit>> {
        self.fetch_if_stale()?;
        let body = serde_json::to_string_pretty(ours)?;
        let repo = git2::Repository::open(&self.working_dir)?;
        let own_ref = self.own_ref();
        if last_published(&repo, &own_ref).as_deref() == Some(body.as_str()) {
            return self.read_local();
        }

        let tree = repo.find_tree(repo.treebuilder(None)?.write()?)?;
        let signature = git2::Signature::now("gitalong", "gitalong@localhost")?;
        let oid = repo.commit(None, &signature, &signature, &body, &tree, &[])?;
        let refspec = format!("{oid}:{own_ref}");
        run_git_in(
            &self.working_dir,
            &[
                "push",
                "--quiet",
                "--force",
                "--no-verify",
                &self.remote,
                &refspec,
            ],
        )?;
        repo.reference(&own_ref, oid, true, "gitalong publish")?;
        self.read_local()
    }

    /// Delete this clone's ref on the remote and locally.
    pub fn clear_own(&mut self) -> Result<()> {
        let own_ref = self.own_ref();
        let present = self.remote_refs(&own_ref)?;
        self.delete_remote_refs(&present)?;
        let repo = git2::Repository::open(&self.working_dir)?;
        if let Ok(mut reference) = repo.find_reference(&own_ref) {
            reference.delete()?;
        }
        Ok(())
    }

    /// Delete every ref in the namespace on the remote and locally.
    pub fn clear_all(&mut self) -> Result<()> {
        let present = self.remote_refs(&format!("{NAMESPACE}/*"))?;
        self.delete_remote_refs(&present)?;
        let repo = git2::Repository::open(&self.working_dir)?;
        for reference in repo.references_glob(&format!("{NAMESPACE}/*"))? {
            reference?.delete()?;
        }
        Ok(())
    }

    /// Refs on the remote matching `pattern`, by full name.
    fn remote_refs(&self, pattern: &str) -> Result<Vec<String>> {
        let listing = run_git_in(
            &self.working_dir,
            &["ls-remote", "--refs", &self.remote, pattern],
        )?;
        Ok(String::from_utf8_lossy(&listing.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .map(str::to_string)
            .collect())
    }

    fn delete_remote_refs(&self, refs: &[String]) -> Result<()> {
        if refs.is_empty() {
            return Ok(());
        }
        let mut args = vec![
            "push",
            "--quiet",
            "--no-verify",
            self.remote.as_str(),
            "--delete",
        ];
        args.extend(refs.iter().map(String::as_str));
        run_git_in(&self.working_dir, &args)?;
        Ok(())
    }

    /// Fetch the namespace unless a fetch happened within the cache window.
    /// A failed fetch is not an error; the last fetched view stands.
    fn fetch_if_stale(&self) -> Result<()> {
        if self.fetched_within_threshold() {
            return Ok(());
        }
        let refspec = format!("+{NAMESPACE}/*:{NAMESPACE}/*");
        let fetched = run_git_in(
            &self.working_dir,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--prune",
                &self.remote,
                &refspec,
            ],
        );
        if fetched.is_ok() {
            touch(&self.fetch_marker())?;
        }
        Ok(())
    }

    /// Records from every ref in the namespace this clone has fetched, plus its own.
    fn read_local(&self) -> Result<Vec<Commit>> {
        let repo = git2::Repository::open(&self.working_dir)?;
        let mut out = Vec::new();
        for reference in repo.references_glob(&format!("{NAMESPACE}/*"))? {
            let reference = reference?;
            let name = reference.name().unwrap_or("?").to_string();
            let Some(message) = reference
                .peel_to_commit()
                .ok()
                .and_then(|c| c.message().map(str::to_string))
            else {
                continue;
            };
            let records: Vec<Commit> = serde_json::from_str(&message)
                .map_err(|e| Error::InvalidConfig(format!("malformed records at {name}: {e}")))?;
            out.extend(records);
        }
        Ok(out)
    }

    fn fetch_marker(&self) -> PathBuf {
        self.state_dir.join("fetched")
    }

    fn fetched_within_threshold(&self) -> bool {
        match std::fs::metadata(self.fetch_marker()).and_then(|m| m.modified()) {
            Ok(t) => SystemTime::now()
                .duration_since(t)
                .map(|d| d.as_secs_f64() < self.pull_threshold)
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

/// Message of the commit `own_ref` points at, or `None` when never published.
fn last_published(repo: &git2::Repository, own_ref: &str) -> Option<String> {
    repo.find_reference(own_ref)
        .ok()?
        .peel_to_commit()
        .ok()?
        .message()
        .map(str::to_string)
}

/// Read the persisted clone id, minting one on first use.
fn load_or_mint_clone_id(path: &Path, context: &Context) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(id) if !id.trim().is_empty() => return Ok(id.trim().to_string()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let id = mint_clone_id(context);
    std::fs::write(path, &id)?;
    Ok(id)
}

/// 128 hex bits from the clone's identity, the clock and the pid. Only
/// uniqueness matters, since the id is persisted rather than re-derived.
fn mint_clone_id(context: &Context) -> String {
    let mut hasher = std::hash::DefaultHasher::new();
    context.host.hash(&mut hasher);
    context.user.hash(&mut hasher);
    context.clone.hash(&mut hasher);
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let high = hasher.finish();
    high.hash(&mut hasher);
    let low = hasher.finish();
    format!("{high:016x}{low:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A bare origin plus a managed clone configured for the refs store.
    struct Fixture {
        origin: TempDir,
        managed: TempDir,
    }

    fn clone_of(origin: &Path) -> TempDir {
        let managed = tempfile::tempdir().unwrap();
        git(
            managed.path(),
            &[
                "clone",
                "--quiet",
                origin.to_str().unwrap(),
                managed.path().to_str().unwrap(),
            ],
        );
        Config {
            store_url: String::new(),
            pull_threshold: 0.0,
            ..Config::default()
        }
        .save(&managed.path().join(CONFIG_BASENAME))
        .unwrap();
        managed
    }

    fn make_fixture() -> Fixture {
        let origin = tempfile::tempdir().unwrap();
        git(
            origin.path(),
            &["init", "--quiet", "--bare", "--initial-branch=main"],
        );
        let managed = clone_of(origin.path());
        Fixture { origin, managed }
    }

    fn open(dir: &Path) -> RefStore {
        RefStore::new(&Repository::open(dir).unwrap()).unwrap()
    }

    fn record(sha: &str) -> Commit {
        Commit {
            sha: Some(sha.into()),
            ..Commit::default()
        }
    }

    #[test]
    fn read_on_empty_remote_is_empty() {
        let f = make_fixture();
        assert!(open(f.managed.path()).read().unwrap().is_empty());
    }

    #[test]
    fn publish_then_read_round_trips() {
        let f = make_fixture();
        let mut store = open(f.managed.path());
        let ours = vec![
            record("a"),
            Commit {
                user: Some("alice".into()),
                changes: vec!["draft.txt".into()],
                ..Commit::default()
            },
        ];
        assert_eq!(store.publish(&ours).unwrap(), ours);
        assert_eq!(store.read().unwrap(), ours);
    }

    #[test]
    fn second_clone_sees_first_clones_records() {
        let f = make_fixture();
        let bob = clone_of(f.origin.path());
        open(f.managed.path())
            .publish(&[record("from-alice")])
            .unwrap();
        let seen = open(bob.path()).read().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].sha.as_deref(), Some("from-alice"));
    }

    #[test]
    fn republish_replaces_only_own_records() {
        let f = make_fixture();
        let bob = clone_of(f.origin.path());
        let mut alice = open(f.managed.path());
        alice.publish(&[record("alice-1")]).unwrap();
        open(bob.path()).publish(&[record("bob-1")]).unwrap();

        let view = alice.publish(&[record("alice-2")]).unwrap();
        let mut shas: Vec<&str> = view.iter().filter_map(|c| c.sha.as_deref()).collect();
        shas.sort();
        assert_eq!(shas, vec!["alice-2", "bob-1"]);
    }

    #[test]
    fn published_commit_carries_only_metadata() {
        let f = make_fixture();
        let mut store = open(f.managed.path());
        store.publish(&[record("a")]).unwrap();

        let oid = git(f.origin.path(), &["rev-parse", &store.own_ref()]);
        let tree = git(f.origin.path(), &["rev-parse", &format!("{oid}^{{tree}}")]);
        assert_eq!(
            tree, "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "records must hang off the empty tree"
        );
        let parents = git(f.origin.path(), &["rev-list", "--parents", "-n1", &oid]);
        assert_eq!(parents, oid, "each publish is a root commit");
        assert!(
            git(f.origin.path(), &["branch", "--list"]).is_empty(),
            "publishing must not create a branch"
        );
    }

    #[test]
    fn publish_skips_push_when_unchanged() {
        let f = make_fixture();
        let mut store = open(f.managed.path());
        store.publish(&[record("a")]).unwrap();

        git(
            f.managed.path(),
            &["remote", "set-url", "origin", "/nonexistent"],
        );
        store
            .publish(&[record("a")])
            .expect("unchanged records need no push");
        assert!(
            store.publish(&[record("b")]).is_err(),
            "changed records must push"
        );
    }

    #[test]
    fn clear_all_empties_the_namespace_on_the_remote() {
        let f = make_fixture();
        let bob = clone_of(f.origin.path());
        open(f.managed.path()).publish(&[record("a")]).unwrap();
        open(bob.path()).publish(&[record("b")]).unwrap();
        assert_eq!(
            git(f.origin.path(), &["for-each-ref", "refs/gitalong/v1/"])
                .lines()
                .count(),
            2
        );

        open(f.managed.path()).clear_all().unwrap();
        assert!(git(f.origin.path(), &["for-each-ref", "refs/gitalong/v1/"]).is_empty());
        assert!(git(f.managed.path(), &["for-each-ref", "refs/gitalong/v1/"]).is_empty());
    }

    #[test]
    fn clone_id_persists_and_differs_between_clones() {
        let f = make_fixture();
        let first = open(f.managed.path()).own_ref();
        assert_eq!(open(f.managed.path()).own_ref(), first);
        let bob = clone_of(f.origin.path());
        assert_ne!(open(bob.path()).own_ref(), first);
    }

    #[test]
    fn new_fails_without_a_remote() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        Config::default()
            .save(&dir.path().join(CONFIG_BASENAME))
            .unwrap();
        assert!(RefStore::new(&Repository::open(dir.path()).unwrap()).is_err());
    }
}
