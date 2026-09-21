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
    use std::path::PathBuf;

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
