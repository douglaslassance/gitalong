//! Command-line surface for `git-along`.
//!
//! The CLI mirrors the Python 0.x contract: same subcommands and long option
//! names. Short options follow standard clap conventions (single character)
//! rather than the click-style multi-character shorts the Python version used.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Top-level CLI parser.
#[derive(Debug, Parser)]
#[command(
    name = "git-along",
    bin_name = "git along",
    version,
    about = "Keep your team in sync on what's being worked on across Git clones.",
    propagate_version = true
)]
pub struct Cli {
    /// Path of the repository to operate on (defaults to current directory).
    #[arg(short = 'C', long = "repository", global = true, value_name = "PATH")]
    pub repository: Option<PathBuf>,

    /// Path to the `git` binary (defaults to whatever is on PATH).
    #[arg(long = "git-binary", global = true, value_name = "PATH")]
    pub git_binary: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands recognized by `git-along`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the Git Along version.
    Version,

    /// Print the value of a configuration property from `.alongconfig`.
    Config {
        /// Property name (e.g. `store-url`, `track-binaries`). Hyphens map to underscores.
        property: String,
    },

    /// Initialize Git Along in a repository.
    Setup(SetupArgs),

    /// Update the store with this clone's local changes.
    Update {
        /// Write a `git-along.prof` profile dump for analysis.
        #[arg(short, long)]
        profile: bool,
    },

    /// Show tracking status for the given files.
    Status {
        /// File paths (relative to the repository or absolute).
        files: Vec<PathBuf>,

        /// Emit one JSON array instead of the whitespace-delimited text lines.
        #[arg(long)]
        json: bool,

        /// Write a `git-along.prof` profile dump for analysis.
        #[arg(short, long)]
        profile: bool,
    },

    /// Claim files for editing, blocking on contested ones.
    Claim {
        /// File paths (relative to the repository or absolute).
        files: Vec<PathBuf>,

        /// Emit one JSON array instead of the whitespace-delimited text lines.
        #[arg(long)]
        json: bool,

        /// Write a `git-along.prof` profile dump for analysis.
        #[arg(short, long)]
        profile: bool,
    },

    /// Remove this clone's records from the store, or every clone's with --all.
    Clear {
        /// Remove every clone's records, not just this clone's.
        #[arg(long)]
        all: bool,

        /// Skip the confirmation that --all asks for.
        #[arg(short, long)]
        force: bool,
    },
}

/// Arguments for `git along setup`.
#[derive(Debug, clap::Args)]
pub struct SetupArgs {
    /// URL of a store repository (`.git` suffix) or a JSONBin.io bin. Omit to
    /// publish records as refs on this repository's own remote.
    pub store_url: Option<String>,

    /// HTTP header for store requests in `KEY=VALUE` form, repeatable.
    ///
    /// Values starting with `$` are expanded from the environment, e.g.
    /// `--store-header X-Access-Key=$JSONBIN_KEY`.
    #[arg(short = 'H', long = "store-header", value_name = "KEY=VALUE")]
    pub store_headers: Vec<String>,

    /// Manage file write permissions to enforce claims at the filesystem level.
    #[arg(short = 'm', long = "modify-permissions")]
    pub modify_permissions: bool,

    /// Cache store pulls for this many seconds (network optimization).
    #[arg(
        long = "pull-threshold",
        default_value_t = 60.0,
        value_name = "SECONDS"
    )]
    pub pull_threshold: f64,

    /// Auto-detect and track all binary files in the repository.
    #[arg(short = 'b', long = "track-binaries")]
    pub track_binaries: bool,

    /// Track uncommitted changes as sha-less commits in the store.
    #[arg(short = 'u', long = "track-uncommitted")]
    pub track_uncommitted: bool,

    /// Comma-separated file extensions to track (e.g. `.jpg,.png`).
    #[arg(
        short = 'e',
        long = "tracked-extensions",
        value_delimiter = ',',
        value_name = "EXT,EXT,..."
    )]
    pub tracked_extensions: Vec<String>,

    /// Install the Git Along git hooks (post-applypatch, post-checkout, post-commit, post-rewrite).
    #[arg(long = "update-hooks")]
    pub update_hooks: bool,
}
