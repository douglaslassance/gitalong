//! High-level workflows over [`Repository`](crate::repository::Repository) and
//! [`Store`](crate::store::Store).
//!
//! The Python implementation expressed these as `async def` functions that
//! parallelized git subprocess calls over `asyncio`. Rust runs them sequentially
//! for now — the working set is small enough that the difference doesn't show
//! in practice, and `rayon::par_iter` is the obvious lever to pull when it does.

pub mod update;

pub use update::update_tracked_commits;
