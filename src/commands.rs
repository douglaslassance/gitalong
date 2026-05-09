//! Per-subcommand entry points.
//!
//! Each function here is the boundary between [`crate::cli`] (parsing) and the
//! library internals. They take parsed args plus shared global flags and
//! return an [`anyhow::Result`] suitable for the binary to bubble up.
//!
//! Every command is currently a stub; functionality will land
//! command-by-command in subsequent commits.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::VERSION;
use crate::cli::SetupArgs;
use crate::repository::Repository;

/// Resolved global flags shared across subcommands.
#[derive(Debug, Clone)]
pub struct GlobalOpts {
    /// Repository the command should operate on. Resolved to the current directory when absent.
    pub repository: PathBuf,
    /// Optional override for the `git` binary path.
    pub git_binary: Option<PathBuf>,
}

impl GlobalOpts {
    /// Resolve global flags into concrete paths, falling back to the current directory.
    pub fn resolve(repository: Option<PathBuf>, git_binary: Option<PathBuf>) -> Result<Self> {
        let repository = match repository {
            Some(p) => p,
            None => std::env::current_dir()?,
        };
        Ok(Self {
            repository,
            git_binary,
        })
    }
}

/// Print `gitalong version X.Y.Z`. Matches the Python 0.x output format.
pub fn version(_opts: &GlobalOpts) -> Result<()> {
    println!("gitalong version {VERSION}");
    Ok(())
}

/// Print a single property from `.gitalong.json`.
///
/// If the path is not in a managed repository or the property is unknown,
/// prints nothing and exits successfully — matching the Python behavior so
/// shell scripts conditionally checking the output keep working.
pub fn config(opts: &GlobalOpts, property: &str) -> Result<()> {
    let Some(repo) = Repository::discover(&opts.repository)? else {
        return Ok(());
    };
    if let Some(value) = repo.config().property(property) {
        println!("{value}");
    }
    Ok(())
}

/// Initialize gitalong in the repository: write config, clone the store, optionally install hooks.
pub fn setup(_opts: &GlobalOpts, _args: SetupArgs) -> Result<()> {
    bail!("`gitalong setup` is not implemented yet");
}

/// Push this clone's local changes to the store, refreshing tracked commits.
pub fn update(_opts: &GlobalOpts, _profile: bool) -> Result<()> {
    bail!("`gitalong update` is not implemented yet");
}

/// Print the tracking status (commit spread) for each given file.
pub fn status(_opts: &GlobalOpts, _files: &[PathBuf], _profile: bool) -> Result<()> {
    bail!("`gitalong status` is not implemented yet");
}

/// Claim files for editing, returning a non-zero exit status when any are blocked.
pub fn claim(_opts: &GlobalOpts, _files: &[PathBuf], _profile: bool) -> Result<()> {
    bail!("`gitalong claim` is not implemented yet");
}

/// Helper for future command implementations: ensure the resolved repository path exists.
#[allow(dead_code)]
pub(crate) fn ensure_repo_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("repository path does not exist: {}", path.display());
    }
    Ok(())
}
