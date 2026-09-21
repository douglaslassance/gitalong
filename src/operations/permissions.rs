//! Filesystem-level enforcement of claims via the read-only attribute.
//!
//! When a repository runs with `modify_permissions = true`, gitalong toggles
//! the user-write bit on each tracked file based on the [`CommitSpread`] of
//! its latest store record:
//!
//! - **Writable**: the record is purely the current clone's responsibility,
//!   either an uncommitted edit or claim (`MINE_UNCOMMITTED`) or an unpushed
//!   commit on the active branch (`MINE_ACTIVE_BRANCH`), and nothing else.
//! - **Read-only**: any other state, including files with no record at all.
//!   Pushed work is public and has to be claimed before editing.
//!
//! Only the store is consulted. The git log fallback `status` uses can only
//! ever yield a read-only answer here, because this clone's unpushed commits
//! are already in the store by the time `update` reaches this step, and
//! walking history per file made `update` take minutes on large trees.
//!
//! This is best-effort: missing files and permission errors are skipped
//! rather than failing the whole operation, matching the Python
//! `_set_write_permission(safe=True)` posture.

use std::path::Path;

use crate::commit::Commit;
use crate::error::Result;
use crate::operations::status::StoreIndex;
use crate::repository::Repository;
use crate::spread::CommitSpread;
use crate::store::Store;

/// Set the user-write bit on each given file according to its tracked status.
///
/// `files` are repo-relative paths. The function returns the list of files
/// whose write bit was changed — useful for the CLI to echo what it touched.
/// The store is expected to already reflect this clone's local view, as it
/// does when `update` calls this after writing.
pub fn update_files_permissions(repo: &Repository, files: &[String]) -> Result<Vec<String>> {
    let mut store = Store::for_repository(repo)?;
    let commits = store.read()?;
    apply_permissions(repo, files, &commits)
}

/// [`update_files_permissions`] against store contents already in hand.
pub(crate) fn apply_permissions(
    repo: &Repository,
    files: &[String],
    commits: &[Commit],
) -> Result<Vec<String>> {
    let remote_url = repo.remote_url()?.unwrap_or_default();
    let index = StoreIndex::new(commits, &remote_url, repo.config().track_uncommitted);
    let head_tree = repo.head_tree()?;
    let active = repo.active_branch_name()?;
    let ctx = repo.context();

    let mut touched = Vec::new();
    for file in files {
        let abs = repo.absolute_path(Path::new(file));
        if !abs.is_file() {
            continue;
        }
        let rel = repo
            .relative_path(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        let tracked = repo.is_file_tracked_in(&abs, head_tree.as_ref())?;
        let want_writable = tracked
            && index.latest(&rel).is_some_and(|c| {
                let spread = c.spread(active.as_deref(), &ctx);
                spread == CommitSpread::MINE_UNCOMMITTED
                    || spread == CommitSpread::MINE_ACTIVE_BRANCH
            });
        if set_writable(&abs, want_writable).unwrap_or(false) {
            touched.push(file.clone());
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
    if perms.readonly() != writable {
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
    use crate::config::{CONFIG_BASENAME, Config};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use tempfile::{TempDir, tempdir};

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Managed clone with permission management on, plus its origin and store.
    fn fixture() -> (TempDir, TempDir, TempDir) {
        let store = tempdir().unwrap();
        run(store.path(), &["init", "--bare", "--initial-branch=main"]);
        let origin = tempdir().unwrap();
        run(origin.path(), &["init", "--bare", "--initial-branch=main"]);
        let managed = tempdir().unwrap();
        run(
            managed.path(),
            &[
                "clone",
                origin.path().to_str().unwrap(),
                managed.path().to_str().unwrap(),
            ],
        );
        run(
            managed.path(),
            &["config", "user.email", "alice@example.com"],
        );
        run(managed.path(), &["config", "user.name", "Alice"]);
        run(managed.path(), &["config", "core.fileMode", "false"]);
        let cfg = Config {
            store_url: format!("file://{}", store.path().display()),
            pull_threshold: 0.0,
            track_uncommitted: true,
            modify_permissions: true,
            ..Config::default()
        };
        cfg.save(&managed.path().join(CONFIG_BASENAME)).unwrap();
        std::fs::write(managed.path().join("README"), b"hi").unwrap();
        std::fs::write(
            managed.path().join(".gitignore"),
            crate::hooks::GITIGNORE_PATCH,
        )
        .unwrap();
        run(
            managed.path(),
            &["add", "README", ".gitalong.json", ".gitignore"],
        );
        run(managed.path(), &["commit", "-m", "init"]);
        run(managed.path(), &["push", "-u", "origin", "main"]);
        (store, origin, managed)
    }

    fn is_writable(path: &Path) -> bool {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o200 != 0
    }

    #[test]
    fn update_locks_pushed_files_and_frees_this_clones_work() {
        let (_s, _o, m) = fixture();
        std::fs::write(m.path().join("local.txt"), b"mine").unwrap();
        run(m.path(), &["add", "local.txt"]);
        run(m.path(), &["commit", "-m", "local"]);

        let repo = Repository::open(m.path()).unwrap();
        crate::operations::update_tracked_commits(&repo, &[]).unwrap();
        assert!(
            !is_writable(&m.path().join("README")),
            "pushed file must be read-only until claimed"
        );
        assert!(
            is_writable(&m.path().join("local.txt")),
            "unpushed commit on the active branch stays writable"
        );

        crate::operations::update_tracked_commits(&repo, &["README".to_string()]).unwrap();
        assert!(
            is_writable(&m.path().join("README")),
            "claimed file becomes writable"
        );
    }

    #[test]
    fn set_writable_toggles_user_write_bit() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, b"x").unwrap();

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
        assert!(!set_writable(&p, true).unwrap());
    }
}
