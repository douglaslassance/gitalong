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

/// Backend dispatch over the available store flavors.
///
/// Modeled as an enum rather than a `Box<dyn>` so static dispatch keeps the
/// hot read/write path allocation-free.
pub enum Store {
    Refs(RefStore),
    Git(GitStore),
    Jsonbin(JsonbinStore),
}

impl Store {
    /// Build the store backend selected by the repository's `store_url`. An
    /// empty URL selects the refs store on the repository's own remote.
    pub fn for_repository(repo: &Repository) -> Result<Self> {
        let url = &repo.config().store_url;
        if url.is_empty() {
            Ok(Store::Refs(RefStore::new(repo)?))
        } else if url.ends_with(".git") || url.starts_with("file://") {
            Ok(Store::Git(GitStore::open_or_clone(repo)?))
        } else if url.starts_with("http://") || url.starts_with("https://") {
            Ok(Store::Jsonbin(JsonbinStore::new(repo)?))
        } else {
            Err(Error::InvalidConfig(format!(
                "store_url `{url}` is neither empty, a `.git` or `file://` URL, nor an HTTP URL"
            )))
        }
    }

    /// Pull (subject to the cache window) and return every clone's records.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        match self {
            Store::Refs(s) => s.read(),
            Store::Git(s) => s.read(),
            Store::Jsonbin(s) => s.read(),
        }
    }

    /// Replace the records this clone issued for `remote_url` with `ours`,
    /// leaving other clones' records alone. Returns the complete view after
    /// the write so callers need no second read.
    pub fn publish(
        &mut self,
        ours: &[Commit],
        context: &Context,
        remote_url: &str,
    ) -> Result<Vec<Commit>> {
        match self {
            Store::Refs(s) => s.publish(ours),
            Store::Git(s) => {
                let next = replace_own(s.read()?, ours, context, remote_url);
                s.write(&next)?;
                Ok(next)
            }
            Store::Jsonbin(s) => {
                let next = replace_own(s.read()?, ours, context, remote_url);
                s.write(&next)?;
                Ok(next)
            }
        }
    }

    /// Remove the records this clone issued for `remote_url`, keeping
    /// everyone else's.
    pub fn clear_own(&mut self, context: &Context, remote_url: &str) -> Result<()> {
        match self {
            Store::Refs(s) => s.clear_own(),
            Store::Git(_) | Store::Jsonbin(_) => self.publish(&[], context, remote_url).map(|_| ()),
        }
    }

    /// Remove every clone's records.
    pub fn clear_all(&mut self) -> Result<()> {
        match self {
            Store::Refs(s) => s.clear_all(),
            // Read first: the shared-document stores can only write on top of
            // what the remote already holds.
            Store::Git(s) => {
                s.read()?;
                s.write(&[])
            }
            Store::Jsonbin(s) => {
                s.read()?;
                s.write(&[])
            }
        }
    }
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
        .filter(|c| c.remote.as_deref() != Some(remote_url) || !c.is_ours(context))
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
        assert_eq!(store.publish(&ours, &ctx, &remote).unwrap(), ours);
        assert_eq!(store.read().unwrap(), ours);
    });

    for_each_store!(second_clone_sees_first_clones_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (ctx, remote) = identity(alice.path());
        open(alice.path())
            .publish(&[own_record(&ctx, &remote, "from-alice")], &ctx, &remote)
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
            .publish(
                &[own_record(&alice_ctx, &remote, "alice-1")],
                &alice_ctx,
                &remote,
            )
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")], &bob_ctx, &remote)
            .unwrap();

        let view = alice_store
            .publish(
                &[own_record(&alice_ctx, &remote, "alice-2")],
                &alice_ctx,
                &remote,
            )
            .unwrap();
        assert_eq!(shas(&view), vec!["alice-2", "bob-1"]);
        assert_eq!(
            shas(&open(bob.path()).read().unwrap()),
            vec!["alice-2", "bob-1"]
        );
    });

    for_each_store!(clear_own_removes_only_this_clones_records, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let bob = team.clone("Bob", |_| {});
        let (alice_ctx, remote) = identity(alice.path());
        let (bob_ctx, _) = identity(bob.path());
        let mut alice_store = open(alice.path());
        alice_store
            .publish(
                &[own_record(&alice_ctx, &remote, "alice-1")],
                &alice_ctx,
                &remote,
            )
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")], &bob_ctx, &remote)
            .unwrap();

        alice_store.clear_own(&alice_ctx, &remote).unwrap();
        assert_eq!(shas(&alice_store.read().unwrap()), vec!["bob-1"]);
        assert_eq!(shas(&open(bob.path()).read().unwrap()), vec!["bob-1"]);
    });

    for_each_store!(clear_own_with_nothing_published_is_a_no_op, |kind| {
        let team = Team::new(kind);
        let alice = team.clone("Alice", |_| {});
        let (ctx, remote) = identity(alice.path());
        let mut store = open(alice.path());
        store.clear_own(&ctx, &remote).unwrap();
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
            .publish(
                &[own_record(&alice_ctx, &remote, "alice-1")],
                &alice_ctx,
                &remote,
            )
            .unwrap();
        open(bob.path())
            .publish(&[own_record(&bob_ctx, &remote, "bob-1")], &bob_ctx, &remote)
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
