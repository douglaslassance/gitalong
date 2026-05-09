//! `gitalong status` — query the most recent tracked commit per file.
//!
//! For each file the caller asks about we want to surface "what's the latest
//! thing anyone has done that touches this file?" — covering both pushed
//! commits visible to git and gitalong's own tracked store of unpushed and
//! uncommitted changes.
//!
//! The Python implementation lived in `batch.get_files_last_commits` and ran
//! the per-file work in parallel via asyncio. The Rust port is sequential for
//! now; with typical CLI invocations passing a handful of files, the latency
//! difference doesn't show.

use std::path::Path;

use crate::commit::Commit;
use crate::error::Result;
use crate::repository::Repository;
use crate::store::Store;

/// Result of looking up a file's status: the file as the user passed it,
/// the most recent commit affecting it (or an empty `Commit` when nothing
/// applies), and that commit's [`CommitSpread`](crate::spread::CommitSpread).
pub struct FileStatus {
    /// Filename as supplied by the caller — preserved so output matches input.
    pub filename: String,
    /// Latest commit touching the file, or default-empty when none.
    pub commit: Commit,
}

/// Find the most recent commit relevant to each path. The returned vector is
/// in the same order as `files`.
pub fn last_commits(repo: &Repository, files: &[String]) -> Result<Vec<FileStatus>> {
    let store_commits = read_store_commits(repo)?;
    let context = repo.context();
    let remote_url = repo.remote_url()?.unwrap_or_default();
    let track_uncommitted = repo.config().track_uncommitted;

    let mut out = Vec::with_capacity(files.len());
    for raw in files {
        let abs = repo.absolute_path(Path::new(raw));
        let rel = repo
            .relative_path(&abs)
            .to_string_lossy()
            .replace('\\', "/");

        if !repo.is_file_tracked(&abs)? {
            out.push(FileStatus {
                filename: raw.clone(),
                commit: Commit::default(),
            });
            continue;
        }

        let from_store =
            pick_latest_store_commit(&store_commits, &rel, &remote_url, track_uncommitted);
        let commit = match from_store {
            Some(c) => c.clone(),
            None => last_commit_from_git(repo, &rel, &remote_url, &context)?.unwrap_or_default(),
        };

        let commit = enrich_branches(repo, commit)?;
        out.push(FileStatus {
            filename: raw.clone(),
            commit,
        });
    }
    Ok(out)
}

/// Format a [`FileStatus`] the same way the Python CLI did:
///
/// ```text
/// <spread> <filename> <sha-or-dash> <local-csv> <remote-csv> <host> <author>
/// ```
///
/// Empty commit yields `-------- <filename> - - - - -`. The 8-character
/// spread bitstring is part of the wire contract — bit order and padding
/// must match across versions for shell scripts that parse this output.
pub fn format_status(
    status: &FileStatus,
    active_branch: Option<&str>,
    ctx: &crate::repository::Context,
) -> String {
    let spread = status.commit.spread(active_branch, ctx);
    let sha = status.commit.sha.as_deref().unwrap_or("-");
    let local = csv_or_dash(&status.commit.branches.local);
    let remote = csv_or_dash(&status.commit.branches.remote);
    let host = status.commit.host.as_deref().unwrap_or("-");
    let author = status
        .commit
        .author
        .as_deref()
        .or(status.commit.user.as_deref())
        .unwrap_or("-");
    format!(
        "{} {} {} {} {} {} {}",
        spread.to_status_string(),
        status.filename,
        sha,
        local,
        remote,
        host,
        author
    )
}

fn csv_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(",")
    }
}

/// Read the store once at the top of `last_commits` so we don't re-pull for
/// every file in the same invocation.
fn read_store_commits(repo: &Repository) -> Result<Vec<Commit>> {
    let mut store = Store::for_repository(repo)?;
    store.read()
}

/// Pick the most recent store commit that mentions `rel_path` in its
/// `changes`, restricted to this remote and (optionally) skipping uncommitted
/// records when the config says we don't track them.
fn pick_latest_store_commit<'a>(
    commits: &'a [Commit],
    rel_path: &str,
    remote_url: &str,
    track_uncommitted: bool,
) -> Option<&'a Commit> {
    commits
        .iter()
        .filter(|c| {
            if !track_uncommitted && c.sha.is_none() {
                return false;
            }
            c.remote.as_deref() == Some(remote_url) && c.changes.iter().any(|p| p == rel_path)
        })
        .max_by(|a, b| a.date.cmp(&b.date))
}

/// Fall back to walking the git log for the file when nothing in the store
/// applies. Mirrors the Python `git.log("--all", "--remotes", "--date-order", "--", file)`
/// invocation, returning the most recent commit touching the file.
fn last_commit_from_git(
    repo: &Repository,
    rel_path: &str,
    remote_url: &str,
    ctx: &crate::repository::Context,
) -> Result<Option<Commit>> {
    let inner = repo.git();
    let mut walk = inner.revwalk()?;
    walk.set_sorting(git2::Sort::TIME)?;
    walk.push_glob("refs/heads/*")?;
    let _ = walk.push_glob("refs/remotes/*");

    let target = std::path::Path::new(rel_path);
    for oid in walk {
        let oid = oid?;
        let git_commit = inner.find_commit(oid)?;
        if commit_touches(inner, &git_commit, target)? {
            return Ok(Some(crate::operations::update::commit_from_git_public(
                repo,
                &git_commit,
                ctx,
                remote_url,
            )?));
        }
    }
    Ok(None)
}

/// `true` when the diff between `c` and its first parent (or the empty tree)
/// touches `target`.
///
/// We never return `false` from the callback even after finding a hit:
/// git2 surfaces an early-exit as an EUSER error rather than a clean stop,
/// so the simpler invariant is "always continue iterating, just record".
/// Commit diffs are small enough that the extra scanning is free.
fn commit_touches(repo: &git2::Repository, c: &git2::Commit<'_>, target: &Path) -> Result<bool> {
    let new_tree = c.tree()?;
    let old_tree = if c.parent_count() == 0 {
        None
    } else {
        Some(c.parent(0)?.tree()?)
    };
    let diff = repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)?;
    let mut hit = false;
    diff.foreach(
        &mut |delta, _| {
            for f in [delta.new_file().path(), delta.old_file().path()]
                .into_iter()
                .flatten()
            {
                if f == target {
                    hit = true;
                }
            }
            true
        },
        None,
        None,
        None,
    )?;
    Ok(hit)
}

/// Populate `branches.local` and `branches.remote` for a commit that has a
/// `sha`. No-op for uncommitted-changes records.
fn enrich_branches(repo: &Repository, mut commit: Commit) -> Result<Commit> {
    if let Some(sha) = commit.sha.clone() {
        if commit.branches.local.is_empty() {
            commit.branches.local = repo.local_branches_containing(&sha)?;
        }
        if commit.branches.remote.is_empty() {
            commit.branches.remote = repo.remote_branches_containing(&sha)?;
        }
    }
    Ok(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {} failed", args.join(" "));
    }

    /// Same shape as the update tests' fixture, exposed here so we can ask
    /// about file status after a few different commit scenarios.
    fn fixture(track_uncommitted: bool) -> (TempDir, TempDir, TempDir) {
        let store = tempfile::tempdir().unwrap();
        run(store.path(), &["init", "--bare", "--initial-branch=main"]);
        let origin = tempfile::tempdir().unwrap();
        run(origin.path(), &["init", "--bare", "--initial-branch=main"]);

        let managed = tempfile::tempdir().unwrap();
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

        let cfg = Config {
            store_url: format!("file://{}", store.path().display()),
            pull_threshold: 0.0,
            track_uncommitted,
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

    #[test]
    fn unknown_file_yields_empty_commit() {
        let (_s, _o, m) = fixture(false);
        let repo = Repository::open(m.path()).unwrap();
        let result = last_commits(&repo, &["does/not/exist.txt".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename, "does/not/exist.txt");
        // No commit means defaults: no sha, no branches.
        assert!(result[0].commit.sha.is_none());
        assert!(result[0].commit.branches.local.is_empty());
    }

    #[test]
    fn pushed_file_resolves_to_its_git_commit() {
        let (_s, _o, m) = fixture(false);
        let repo = Repository::open(m.path()).unwrap();
        let result = last_commits(&repo, &["README".to_string()]).unwrap();
        let commit = &result[0].commit;
        assert!(commit.sha.is_some());
        // Pushed file → commit is on origin/main, so remote-matching-branch
        // membership is populated.
        assert!(commit.branches.remote.iter().any(|b| b == "main"));
    }

    #[test]
    fn local_only_commit_is_picked_up_from_store() {
        // Make a local-only commit, run update to record it, then status
        // should report that store entry as the latest.
        let (_s, _o, m) = fixture(false);
        std::fs::write(m.path().join("draft.txt"), b"draft").unwrap();
        run(m.path(), &["add", "draft.txt"]);
        run(m.path(), &["commit", "-m", "wip"]);

        let repo = Repository::open(m.path()).unwrap();
        crate::operations::update_tracked_commits(&repo, &[]).unwrap();

        let result = last_commits(&repo, &["draft.txt".to_string()]).unwrap();
        let c = &result[0].commit;
        assert!(c.sha.is_some());
        assert!(c.changes.iter().any(|p| p == "draft.txt"));
    }

    #[test]
    fn format_status_renders_dashes_for_empty_commit() {
        let s = FileStatus {
            filename: "x.txt".to_string(),
            commit: Commit::default(),
        };
        let ctx = crate::repository::Context {
            host: "h".into(),
            user: "u".into(),
            clone: std::path::PathBuf::from("/"),
        };
        let line = format_status(&s, Some("main"), &ctx);
        assert_eq!(line, "-------- x.txt - - - - -");
    }
}
