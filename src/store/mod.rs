//! Pluggable storage for tracked commits.
//!
//! Three backends ship with gitalong: [`refs::RefStore`] (records pushed to
//! a hidden ref namespace on the managed repository's own remote, the
//! default), [`git::GitStore`] (a separate git repository cloned to
//! `<managed>/.gitalong/`) and [`jsonbin::JsonbinStore`] (a JSONBin.io bin).
//! Callers go through [`Store`], which dispatches to one of them.

pub mod git;
pub mod jsonbin;
pub mod refs;

use std::path::Path;
use std::process::{Command, Output};

use crate::commit::Commit;
use crate::error::{Error, Result};
use crate::repository::{Context, Repository};

pub use git::GitStore;
pub use jsonbin::JsonbinStore;
pub use refs::RefStore;

/// A store, plus the scope its records are read and written under.
///
/// Scoping is the store's job, not the caller's. Every method returns or
/// replaces only the records belonging to this repository, so callers can
/// treat whatever they get back as relevant.
pub struct Store {
    backend: Backend,
    /// Identity this clone publishes under.
    context: Context,
    /// URL of the managed repository's remote.
    remote_url: String,
}

/// Backend dispatch over the available store flavors.
///
/// Modeled as an enum rather than a `Box<dyn>` so static dispatch keeps the
/// hot read/write path allocation-free.
enum Backend {
    /// Refs on the managed repository's own remote. A namespace there belongs
    /// to exactly one repository, so those records need no further scoping.
    Refs(RefStore),
    /// One shared JSON document, which may serve several repositories. Its
    /// records are told apart by the `remote` each one carries.
    Git(GitStore),
    Jsonbin(JsonbinStore),
}

impl Store {
    /// Build the store backend selected by the repository's `store_url`. An
    /// empty URL selects the refs store on the repository's own remote.
    pub fn for_repository(repo: &Repository) -> Result<Self> {
        let url = &repo.config().store_url;
        let backend = if url.is_empty() {
            Backend::Refs(RefStore::new(repo)?)
        } else if url.ends_with(".git") || url.starts_with("file://") {
            Backend::Git(GitStore::open_or_clone(repo)?)
        } else if url.starts_with("http://") || url.starts_with("https://") {
            Backend::Jsonbin(JsonbinStore::new(repo)?)
        } else {
            return Err(Error::InvalidConfig(format!(
                "store_url `{url}` is neither empty, a `.git` or `file://` URL, nor an HTTP URL"
            )));
        };
        Ok(Self {
            backend,
            context: repo.context(),
            remote_url: repo.remote_url()?.unwrap_or_default(),
        })
    }

    /// Pull (subject to the cache window) and return every clone's records
    /// for this repository.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        let remote = self.remote_url.clone();
        match &mut self.backend {
            Backend::Refs(s) => s.read(),
            Backend::Git(s) => Ok(scoped(s.read()?, &remote)),
            Backend::Jsonbin(s) => Ok(scoped(s.read()?, &remote)),
        }
    }

    /// Replace the records this clone issued with `ours`, leaving other
    /// clones' alone. Returns the view after the write so callers need no
    /// second read.
    pub fn publish(&mut self, ours: &[Commit]) -> Result<Vec<Commit>> {
        let (context, remote) = (self.context.clone(), self.remote_url.clone());
        match &mut self.backend {
            Backend::Refs(s) => s.publish(ours),
            Backend::Git(s) => {
                let next = replace_own(s.read()?, ours, &context, &remote);
                s.write(&next)?;
                Ok(scoped(next, &remote))
            }
            Backend::Jsonbin(s) => {
                let next = replace_own(s.read()?, ours, &context, &remote);
                s.write(&next)?;
                Ok(scoped(next, &remote))
            }
        }
    }

    /// Remove the records this clone issued, keeping everyone else's.
    pub fn clear_own(&mut self) -> Result<()> {
        if let Backend::Refs(s) = &mut self.backend {
            return s.clear_own();
        }
        self.publish(&[]).map(|_| ())
    }

    /// Remove every clone's records for this repository. Records belonging to
    /// other repositories sharing the same store are left in place.
    pub fn clear_all(&mut self) -> Result<()> {
        let remote = self.remote_url.clone();
        match &mut self.backend {
            Backend::Refs(s) => s.clear_all(),
            Backend::Git(s) => {
                let kept = others(s.read()?, &remote);
                s.write(&kept)
            }
            Backend::Jsonbin(s) => {
                let kept = others(s.read()?, &remote);
                s.write(&kept)
            }
        }
    }
}

/// `true` when `commit` was recorded against the repository at `remote_url`.
fn belongs_to(commit: &Commit, remote_url: &str) -> bool {
    commit.remote.as_deref() == Some(remote_url)
}

/// Records belonging to the repository at `remote_url`.
fn scoped(commits: Vec<Commit>, remote_url: &str) -> Vec<Commit> {
    commits
        .into_iter()
        .filter(|c| belongs_to(c, remote_url))
        .collect()
}

/// Records belonging to every repository but the one at `remote_url`.
fn others(commits: Vec<Commit>, remote_url: &str) -> Vec<Commit> {
    commits
        .into_iter()
        .filter(|c| !belongs_to(c, remote_url))
        .collect()
}

/// Drop the records this clone issued for `remote_url` and append `ours`.
fn replace_own(
    existing: Vec<Commit>,
    ours: &[Commit],
    context: &Context,
    remote_url: &str,
) -> Vec<Commit> {
    let mut next: Vec<Commit> = existing
        .into_iter()
        .filter(|c| !belongs_to(c, remote_url) || !c.is_ours(context))
        .collect();
    next.extend(ours.iter().cloned());
    next
}

/// Spawn `git <args>` in `dir` and surface a useful error on non-zero exit.
pub(crate) fn run_git_in(dir: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git").current_dir(dir).args(args).output()?;
    check_status(&output, args)?;
    Ok(output)
}

pub(crate) fn check_status(output: &Output, args: &[&str]) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let cmd = args.join(" ");
    Err(Error::StoreUnreachable(format!(
        "git {cmd} failed: {stderr}"
    )))
}

/// Update `path`'s mtime by writing an empty file (creating it if missing).
pub(crate) fn touch(path: &Path) -> Result<()> {
    std::fs::write(path, b"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::for_each_store;
    use crate::testing::Team;
    use std::path::PathBuf;

    fn open(dir: &Path) -> Store {
        Store::for_repository(&Repository::open(dir).unwrap()).unwrap()
    }

    /// The identity a clone publishes under: its context and remote URL.
    fn identity(dir: &Path) -> (Context, String) {
        let repo = Repository::open(dir).unwrap();
        let remote = repo.remote_url().unwrap().unwrap_or_default();
        (repo.context(), remote)
    }

    fn own_record(ctx: &Context, remote: &str, sha: &str) -> Commit {
        let mut c = Commit {
            sha: Some(sha.into()),
            remote: Some(remote.into()),
            ..Commit::default()
        };
        c.stamp_context(ctx);
        c
    }

    fn shas(view: &[Commit]) -> Vec<&str> {
        let mut shas: Vec<&str> = view.iter().filter_map(|c| c.sha.as_deref()).collect();
        shas.sort();
        shas
    }

    for_each_store!(read_on_empty_store_is_empty, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        assert!(open(alice.path()).read().unwrap().is_empty());
    });

    for_each_store!(publish_then_read_round_trips, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let (ctx, remote) = identity(alice.path());
        let ours = vec![own_record(&ctx, &remote, "a")];
        let mut store = open(alice.path());
        assert_eq!(store.publish(&ours).unwrap(), ours);
        assert_eq!(store.read().unwrap(), ours);
    });

    for_each_store!(second_clone_sees_first_clones_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (ctx, remote) = identity(alice.path());
        open(alice.path())
            .publish(&[own_record(&ctx, &remote, "from-alice")])
            .unwrap();
        assert_eq!(shas(&open(bob.path()).read().unwrap()), vec!["from-alice"]);
    });

    for_each_store!(republish_replaces_only_own_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (alice_ctx, remote) = identity(alice.path());
        let (bob_ctx, _) = identity(bob.path());

        let mut alice_store = open(alice.path());
        alice_store
            .publish(&[own_record(&alice_ctx, &remote, "alice-1")])
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")])
            .unwrap();

        let view = alice_store
            .publish(&[own_record(&alice_ctx, &remote, "alice-2")])
            .unwrap();
        assert_eq!(shas(&view), vec!["alice-2", "bob-1"]);
        assert_eq!(
            shas(&open(bob.path()).read().unwrap()),
            vec!["alice-2", "bob-1"]
        );
    });

    /// The refs namespace belongs to one repository, so it must not filter
    /// on the remote URL: two clones spelling the same origin differently
    /// would otherwise be invisible to each other.
    #[test]
    fn refs_store_ignores_how_each_clone_spells_the_origin() {
        let team = Team::new(crate::testing::StoreKind::Refs);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone_via("Bob", &team.origin_file_url(), |_| {});
        let (alice_ctx, alice_remote) = identity(alice.path());
        let (_, bob_remote) = identity(bob.path());
        assert_ne!(
            alice_remote, bob_remote,
            "the fixture must differ in spelling"
        );

        open(alice.path())
            .publish(&[own_record(&alice_ctx, &alice_remote, "from-alice")])
            .unwrap();
        assert_eq!(shas(&open(bob.path()).read().unwrap()), vec!["from-alice"]);
    }

    for_each_store!(clear_own_removes_only_this_clones_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (alice_ctx, remote) = identity(alice.path());
        let (bob_ctx, _) = identity(bob.path());
        let mut alice_store = open(alice.path());
        alice_store
            .publish(&[own_record(&alice_ctx, &remote, "alice-1")])
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")])
            .unwrap();

        alice_store.clear_own().unwrap();
        assert_eq!(shas(&alice_store.read().unwrap()), vec!["bob-1"]);
        assert_eq!(shas(&open(bob.path()).read().unwrap()), vec!["bob-1"]);
    });

    for_each_store!(clear_own_with_nothing_published_is_a_no_op, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let mut store = open(alice.path());
        store.clear_own().unwrap();
        assert!(store.read().unwrap().is_empty());
    });

    for_each_store!(clear_all_removes_every_clones_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (alice_ctx, remote) = identity(alice.path());
        let (bob_ctx, _) = identity(bob.path());
        let mut alice_store = open(alice.path());
        alice_store
            .publish(&[own_record(&alice_ctx, &remote, "alice-1")])
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")])
            .unwrap();

        alice_store.clear_all().unwrap();
        assert!(alice_store.read().unwrap().is_empty());
        assert!(open(bob.path()).read().unwrap().is_empty());
    });

    fn ctx() -> Context {
        Context {
            host: "host-1".into(),
            user: "alice".into(),
            clone: PathBuf::from("/work/repo"),
        }
    }

    fn record(clone: &str, remote: &str, path: &str) -> Commit {
        Commit {
            clone: Some(clone.into()),
            remote: Some(remote.into()),
            changes: vec![path.into()],
            ..Commit::default()
        }
    }

    #[test]
    fn replace_own_keeps_other_clones_and_other_remotes() {
        let existing = vec![
            record("/work/repo", "r", "stale.txt"),
            record("/elsewhere", "r", "theirs.txt"),
            record("/work/repo", "other-remote", "elsewhere.txt"),
        ];
        let ours = vec![record("/work/repo", "r", "fresh.txt")];
        let next = replace_own(existing, &ours, &ctx(), "r");
        let paths: Vec<&str> = next.iter().map(|c| c.changes[0].as_str()).collect();
        assert_eq!(paths, vec!["theirs.txt", "elsewhere.txt", "fresh.txt"]);
    }
}
