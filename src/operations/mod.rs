//! High-level workflows over [`Repository`](crate::repository::Repository) and
//! [`Store`](crate::store::Store).
//!
//! The Python implementation expressed these as `async def` functions that
//! parallelized git subprocess calls over `asyncio`. Rust runs them sequentially
//! for now — the working set is small enough that the difference doesn't show
//! in practice, and `rayon::par_iter` is the obvious lever to pull when it does.

pub mod claim;
pub mod permissions;
pub mod status;
pub mod update;

pub use claim::{ClaimOutcome, claim_files};
pub use permissions::update_files_permissions;
pub use status::{FileStatus, format_status, last_commits};
pub use update::update_tracked_commits;
