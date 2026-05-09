//! Per-subcommand entry points.
//!
//! Each function here is the boundary between [`crate::cli`] (parsing) and the
//! library internals. They take parsed args plus shared global flags and
//! return an [`anyhow::Result`] suitable for the binary to bubble up.
//!
//! Every command is currently a stub; functionality will land
//! command-by-command in subsequent commits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use crate::VERSION;
use crate::cli::SetupArgs;
use crate::config::{CONFIG_BASENAME, Config};
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
///
/// The actual store clone (for git stores) or remote handshake (for jsonbin
/// stores) is deferred to first use. Setup does the local-side work: validate
/// the URL shape, write `.gitalong.json`, and apply the optional flags
/// (`--update-hooks`, `--update-gitignore`, `--modify-permissions`).
pub fn setup(opts: &GlobalOpts, args: SetupArgs) -> Result<()> {
    classify_store_url(&args.store_url)
        .with_context(|| format!("invalid store URL `{}`", args.store_url))?;

    let inner = git2::Repository::discover(&opts.repository)
        .with_context(|| format!("not in a git repository: {}", opts.repository.display()))?;
    let working_dir = inner
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repositories are not supported"))?
        .to_path_buf();

    let config = Config {
        store_url: args.store_url,
        store_headers: parse_store_headers(&args.store_headers)?,
        modify_permissions: args.modify_permissions,
        track_binaries: args.track_binaries,
        tracked_extensions: args.tracked_extensions,
        pull_threshold: args.pull_threshold,
        track_uncommitted: args.track_uncommitted,
    };
    config.save(&working_dir.join(CONFIG_BASENAME))?;

    let repo = Repository::open(&working_dir)?;
    if args.modify_permissions {
        repo.disable_file_mode_tracking()
            .context("failed to disable core.fileMode")?;
    }
    if args.update_gitignore {
        repo.update_gitignore()
            .context("failed to update .gitignore")?;
    }
    if args.update_hooks {
        repo.install_hooks().context("failed to install hooks")?;
    }
    Ok(())
}

/// Decide which store backend a URL points at without instantiating it.
///
/// Returns the matching [`StoreKind`] or an error message for callers to
/// surface. Mirrors the dispatch the Python `Repository.__init__` did inline.
pub(crate) fn classify_store_url(url: &str) -> Result<StoreKind> {
    if url.starts_with("https://api.jsonbin.io") {
        Ok(StoreKind::Jsonbin)
    } else if url.ends_with(".git") {
        Ok(StoreKind::Git)
    } else {
        bail!("expected a `.git` URL or a `https://api.jsonbin.io/...` URL")
    }
}

/// Discriminant for the store backend a config points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    Git,
    Jsonbin,
}

/// Parse `KEY=VALUE` pairs from `--store-header` flags into a map. Duplicate
/// keys are last-write-wins, matching how multi-value CLI flags usually behave.
fn parse_store_headers(raw: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for entry in raw {
        let (k, v) = entry
            .split_once('=')
            .with_context(|| format!("--store-header `{entry}` is not in KEY=VALUE form"))?;
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

/// Push this clone's local changes to the store, refreshing tracked commits.
///
/// `--profile` is a no-op for now; it was wired in the Python CLI to
/// dump a cProfile trace and there's no Rust equivalent on the hot path
/// yet. The flag is kept on the CLI so existing scripts don't break.
pub fn update(opts: &GlobalOpts, _profile: bool) -> Result<()> {
    let Some(repo) = Repository::discover(&opts.repository)? else {
        bail!("not in a managed repository (no .gitalong.json found)");
    };
    crate::operations::update_tracked_commits(&repo, &[])?;
    Ok(())
}

/// Print the tracking status (commit spread) for each given file.
pub fn status(opts: &GlobalOpts, files: &[PathBuf], _profile: bool) -> Result<()> {
    let Some(repo) = Repository::discover(&opts.repository)? else {
        bail!("not in a managed repository (no .gitalong.json found)");
    };
    let str_files: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let statuses = crate::operations::last_commits(&repo, &str_files)?;
    let active = repo.active_branch_name()?;
    let ctx = repo.context();
    for status in &statuses {
        println!("{}", crate::operations::format_status(status, active.as_deref(), &ctx));
    }
    Ok(())
}

/// Claim files for editing, returning a non-zero exit status when any are blocked.
pub fn claim(opts: &GlobalOpts, files: &[PathBuf], _profile: bool) -> Result<()> {
    let Some(repo) = Repository::discover(&opts.repository)? else {
        bail!("not in a managed repository (no .gitalong.json found)");
    };
    let str_files: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let outcomes = crate::operations::claim_files(&repo, &str_files)?;
    let active = repo.active_branch_name()?;
    let ctx = repo.context();

    let mut any_blocked = false;
    for outcome in &outcomes {
        let pseudo_status = crate::operations::FileStatus {
            filename: outcome.filename.clone(),
            commit: outcome.blocker.clone(),
        };
        println!(
            "{}",
            crate::operations::format_status(&pseudo_status, active.as_deref(), &ctx)
        );
        if outcome.blocker.sha.is_some() || outcome.blocker.user.is_some() {
            any_blocked = true;
        }
    }
    if any_blocked {
        // Match the Python contract: exit 1 when any file couldn't be claimed.
        std::process::exit(1);
    }
    Ok(())
}

/// Helper for future command implementations: ensure the resolved repository path exists.
#[allow(dead_code)]
pub(crate) fn ensure_repo_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("repository path does not exist: {}", path.display());
    }
    Ok(())
}
