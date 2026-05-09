//! Typed errors for the gitalong library.
//!
//! The binary uses `anyhow::Result` for ergonomic error chaining at the CLI
//! boundary; library code returns `gitalong::Result` so callers can match on
//! the variants below.

use std::path::PathBuf;

/// Result type aliased for the gitalong library.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors surfaced by the gitalong library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `.gitalong.json` was not found in the repository.
    #[error("gitalong is not set up in {0:?} (no .gitalong.json found)")]
    NotSetup(PathBuf),

    /// `.gitalong.json` could not be parsed or has an invalid value.
    #[error("invalid gitalong configuration: {0}")]
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
