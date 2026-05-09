//! gitalong — keep your team in sync on what's being worked on across Git clones.
//!
//! This crate exposes the gitalong library surface used by the `gitalong` binary
//! and by integration tests. The CLI lives in [`cli`], typed errors in [`error`],
//! and per-command implementations in [`commands`].

pub mod cli;
pub mod commands;
pub mod config;
pub mod error;

pub use config::Config;

pub use error::{Error, Result};

/// Crate version, sourced from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
