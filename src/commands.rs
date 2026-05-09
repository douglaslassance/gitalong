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
use crate::config::Config;
use crate::error::Error;

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
/// Walks up from the resolved repository path looking for the config file. If
/// gitalong is not set up (no config found) or the property is unknown, prints
/// nothing and exits successfully — matching the Python behavior so existing
/// shell scripts that conditionally check output keep working.
pub fn config(opts: &GlobalOpts, property: &str) -> Result<()> {
    let Some(config_path) = find_config_upwards(&opts.repository) else {
        return Ok(());
    };
    let config = match Config::load(&config_path) {
        Ok(c) => c,
        // Race against the filesystem: config disappeared between find and
        // load. Treat the same as "not set up".
        Err(Error::NotSetup(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if let Some(value) = config.property(property) {
        println!("{value}");
    }
    Ok(())
}

/// Walk up from `start` looking for a directory containing `.gitalong.json`.
/// Returns the path to the config file when found.
fn find_config_upwards(start: &Path) -> Option<PathBuf> {
    let mut cursor: Option<&Path> = Some(start);
    while let Some(dir) = cursor {
        let candidate = Config::path_for(dir);
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
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
