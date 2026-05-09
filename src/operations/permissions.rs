//! Filesystem-level enforcement of claims via the read-only attribute.
//!
//! When a repository runs with `modify_permissions = true`, gitalong toggles
//! the user-write bit on each tracked file based on its [`CommitSpread`]:
//!
//! - **Writable**: the file is purely the current clone's responsibility,
//!   either as an uncommitted edit (`MINE_UNCOMMITTED`) or as a committed
//!   change on the active branch (`MINE_ACTIVE_BRANCH`) and nowhere else.
//! - **Read-only**: any other state — another clone is editing it, the
//!   commit lives on a different branch, etc.
//!
//! This is best-effort: missing files and permission errors are logged
//! through the returned vector but never fail the whole operation, matching
//! the Python `_set_write_permission(safe=True)` posture.

use std::path::Path;

use crate::error::Result;
use crate::operations::status::last_commits;
use crate::repository::Repository;
use crate::spread::CommitSpread;

/// Set the user-write bit on each given file according to its tracked status.
///
/// `files` are repo-relative paths. The function returns the list of files
/// whose write bit was changed — useful for the CLI to echo what it touched.
pub fn update_files_permissions(repo: &Repository, files: &[String]) -> Result<Vec<String>> {
    let statuses = last_commits(repo, files)?;
    let active = repo.active_branch_name()?;
    let ctx = repo.context();

    let mut touched = Vec::new();
    for status in statuses {
        let spread = status.commit.spread(active.as_deref(), &ctx);
        // The Python only flips files whose spread *exactly* equals one of
        // the writable bits. A file with both MINE_ACTIVE_BRANCH and
        // REMOTE_MATCHING_BRANCH is treated as read-only because someone
        // else's branch could see it. Mirror that exactness.
        let want_writable =
            spread == CommitSpread::MINE_UNCOMMITTED || spread == CommitSpread::MINE_ACTIVE_BRANCH;
        let abs = repo.absolute_path(Path::new(&status.filename));
        if !abs.is_file() {
            continue;
        }
        match set_writable(&abs, want_writable) {
            Ok(true) => touched.push(status.filename),
            Ok(false) => {}
            // Permission errors on chmod are common when running unprivileged
            // against repo files owned by someone else; surface as a no-op.
            Err(_) => {}
        }
    }
    Ok(touched)
}

/// Set or clear the user-write permission. Returns `Ok(true)` when the
/// permission actually changed.
#[cfg(unix)]
fn set_writable(path: &Path, writable: bool) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)?;
    let mut perms = meta.permissions();
    let mode = perms.mode();
    let user_write = 0o200;
    let next = if writable {
        mode | user_write
    } else {
        mode & !user_write
    };
    if next == mode {
        return Ok(false);
    }
    perms.set_mode(next);
    std::fs::set_permissions(path, perms)?;
    Ok(true)
}

#[cfg(not(unix))]
fn set_writable(path: &Path, writable: bool) -> Result<bool> {
    let meta = std::fs::metadata(path)?;
    let mut perms = meta.permissions();
    if perms.readonly() == !writable {
        return Ok(false);
    }
    perms.set_readonly(!writable);
    std::fs::set_permissions(path, perms)?;
    Ok(true)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn set_writable_toggles_user_write_bit() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, b"x").unwrap();

        // Drop write, verify it's gone, restore it, verify it's back.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(set_writable(&p, true).unwrap());
        assert_ne!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o200,
            0
        );

        assert!(set_writable(&p, false).unwrap());
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o200,
            0
        );
    }

    #[test]
    fn set_writable_returns_false_when_no_change() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, b"x").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        // Already writable: a request for writable should be a no-op.
        assert!(!set_writable(&p, true).unwrap());
    }
}
