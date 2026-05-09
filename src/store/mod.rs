//! Pluggable storage for tracked commits.
//!
//! Two backends ship with gitalong: [`git::GitStore`] (a regular git
//! repository cloned to `<managed>/.gitalong/`) and a future
//! `JsonbinStore`. Callers go through [`Store`], which dispatches to one
//! or the other.

pub mod git;
pub mod jsonbin;

use crate::commit::Commit;
use crate::error::{Error, Result};
use crate::repository::Repository;

pub use git::GitStore;
pub use jsonbin::JsonbinStore;

/// Backend dispatch over the available store flavors.
///
/// Modeled as an enum rather than a `Box<dyn>` so static dispatch keeps the
/// hot read/write path allocation-free.
pub enum Store {
    Git(GitStore),
    Jsonbin(JsonbinStore),
}

impl Store {
    /// Build the store backend selected by the repository's `store_url`.
    pub fn for_repository(repo: &Repository) -> Result<Self> {
        let url = &repo.config().store_url;
        if url.starts_with("https://api.jsonbin.io") {
            Ok(Store::Jsonbin(JsonbinStore::new(repo)?))
        } else if url.ends_with(".git") || url.starts_with("file://") {
            Ok(Store::Git(GitStore::open_or_clone(repo)?))
        } else {
            Err(Error::InvalidConfig(format!(
                "store_url `{url}` is neither a `.git` URL nor a JSONBin URL"
            )))
        }
    }

    /// Pull (subject to the cache window) and return all tracked commits.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        match self {
            Store::Git(s) => s.read(),
            Store::Jsonbin(s) => s.read(),
        }
    }

    /// Persist `commits` to the store. The git backend commits and pushes;
    /// the jsonbin backend issues an HTTP PUT.
    pub fn write(&mut self, commits: &[Commit]) -> Result<()> {
        match self {
            Store::Git(s) => s.write(commits),
            Store::Jsonbin(s) => s.write(commits),
        }
    }
}
