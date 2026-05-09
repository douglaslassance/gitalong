//! Pluggable storage for tracked commits.
//!
//! Two backends ship with gitalong: [`git::GitStore`] (a regular git
//! repository cloned to `<managed>/.gitalong/`) and a future
//! `JsonbinStore`. Callers go through [`Store`], which dispatches to one
//! or the other.

pub mod git;

use crate::commit::Commit;
use crate::error::Result;
use crate::repository::Repository;

pub use git::GitStore;

/// Backend dispatch over the two store flavors.
///
/// Modeled as an enum rather than a `Box<dyn>` so static dispatch keeps the
/// hot read/write path allocation-free.
pub enum Store {
    Git(GitStore),
}

impl Store {
    /// Build the store backend selected by the repository's `store_url`.
    pub fn for_repository(repo: &Repository) -> Result<Self> {
        // Future: pick GitStore vs JsonbinStore based on URL shape. Until the
        // jsonbin backend lands, anything classified as a git store works,
        // and anything else is rejected up front.
        Ok(Store::Git(GitStore::open_or_clone(repo)?))
    }

    /// Pull (subject to the cache window) and return all tracked commits.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        match self {
            Store::Git(s) => s.read(),
        }
    }

    /// Persist `commits` to the store. The git backend commits and pushes;
    /// future backends may issue a remote PUT.
    pub fn write(&mut self, commits: &[Commit]) -> Result<()> {
        match self {
            Store::Git(s) => s.write(commits),
        }
    }
}
