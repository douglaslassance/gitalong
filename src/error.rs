//! Typed errors for the Git Along library.
//!
//! The binary uses `anyhow::Result` for ergonomic error chaining at the CLI
//! boundary; library code returns `git_along::Result` so callers can match on
//! the variants below.

use std::path::PathBuf;

/// Result type aliased for the Git Along library.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors surfaced by the Git Along library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `.along.json` was not found in the repository.
    #[error("Git Along is not set up in {0:?} (no .along.json found)")]
    NotSetup(PathBuf),

    /// `.along.json` could not be parsed or has an invalid value.
    #[error("invalid Git Along configuration: {0}")]
    InvalidConfig(String),

    /// The store backend could not be reached.
    #[error("store unreachable: {0}")]
    StoreUnreachable(String),

    /// I/O failure (filesystem, subprocess, etc.).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON serialization or parsing failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Underlying git operation failed.
    #[error(transparent)]
    Git(#[from] git2::Error),
}
