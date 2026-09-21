//! `gitalong status` — query the most recent tracked commit per file.
//!
//! For each file the caller asks about we want to surface "what's the latest
//! thing anyone has done that touches this file?" — covering both pushed
//! commits visible to git and gitalong's own tracked store of unpushed and
//! uncommitted changes.
//!
//! The Python implementation lived in `batch.get_files_last_commits` and ran
//! the per-file work in parallel via asyncio. The Rust port instead does the
//! expensive work once per invocation: the store is indexed by path, the HEAD
//! tree is peeled once, and every file the store knows nothing about is
//! resolved by a single walk of the history.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::commit::{Branches, Commit};
use crate::error::Result;
use crate::repository::{Context, Repository};
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
    let index = StoreIndex::new(&store_commits, &remote_url, repo.config().track_uncommitted);
    let head_tree = repo.head_tree()?;

    let mut commits: Vec<Commit> = Vec::with_capacity(files.len());
    let mut pending: Vec<(usize, String)> = Vec::new();
    for (i, raw) in files.iter().enumerate() {
        let abs = repo.absolute_path(Path::new(raw));
        let rel = repo
            .relative_path(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        if !repo.is_file_tracked_in(&abs, head_tree.as_ref())? {
            commits.push(Commit::default());
            continue;
        }
        match index.latest(&rel) {
            Some(c) => commits.push(c.clone()),
            None => {
                pending.push((i, rel));
                commits.push(Commit::default());
            }
        }
    }

    if !pending.is_empty() {
        let rels: Vec<&str> = pending.iter().map(|(_, rel)| rel.as_str()).collect();
        let found = last_commits_from_git(repo, &rels, &remote_url, &context)?;
        for (i, rel) in &pending {
            if let Some(c) = found.get(rel.as_str()) {
                commits[*i] = c.clone();
            }
        }
    }

    files
        .iter()
        .zip(commits)
        .map(|(raw, commit)| {
            Ok(FileStatus {
                filename: raw.clone(),
                commit: enrich_branches(repo, commit)?,
            })
        })
        .collect()
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

/// The most recent store record per path, restricted to this remote and
/// (optionally) skipping uncommitted records when the config says we don't
/// track them. Built once so each lookup is a hash probe instead of a scan.
pub(crate) struct StoreIndex<'a> {
    latest: HashMap<&'a str, &'a Commit>,
}

impl<'a> StoreIndex<'a> {
    pub(crate) fn new(commits: &'a [Commit], remote_url: &str, track_uncommitted: bool) -> Self {
        let mut latest: HashMap<&str, &Commit> = HashMap::new();
        for c in commits {
            if !track_uncommitted && c.sha.is_none() {
                continue;
            }
            if c.remote.as_deref() != Some(remote_url) {
                continue;
            }
            for path in &c.changes {
                let newer = latest
                    .get(path.as_str())
                    .is_none_or(|prev| prev.date <= c.date);
                if newer {
                    latest.insert(path.as_str(), c);
                }
            }
        }
        Self { latest }
    }

    /// Latest record mentioning `rel_path`, or `None` when the store has nothing for it.
    pub(crate) fn latest(&self, rel_path: &str) -> Option<&'a Commit> {
        self.latest.get(rel_path).copied()
    }
}

/// Fall back to the git log for every path the store knows nothing about,
/// in a single walk. Mirrors the Python
/// `git.log("--all", "--remotes", "--date-order", "--", file)` per file,
/// returning the most recent commit touching each path that has one.
fn last_commits_from_git(
    repo: &Repository,
    rel_paths: &[&str],
    remote_url: &str,
    ctx: &Context,
) -> Result<HashMap<String, Commit>> {
    let inner = repo.git();
    let mut walk = inner.revwalk()?;
    walk.set_sorting(git2::Sort::TIME)?;
    walk.push_glob("refs/heads/*")?;
    let _ = walk.push_glob("refs/remotes/*");

    let mut pending: Vec<PathBuf> = rel_paths.iter().map(PathBuf::from).collect();
    pending.sort();
    pending.dedup();

    let mut found = HashMap::new();
    for oid in walk {
        if pending.is_empty() {
            break;
        }
        let git_commit = inner.find_commit(oid?)?;
        let tree = git_commit.tree()?;
        let parent_tree = match git_commit.parent_count() {
            0 => None,
            _ => Some(git_commit.parent(0)?.tree()?),
        };
        let (hits, rest): (Vec<PathBuf>, Vec<PathBuf>) = pending.into_iter().partition(|path| {
            entry_signature(&tree, path)
                != parent_tree.as_ref().and_then(|t| entry_signature(t, path))
        });
        pending = rest;
        if hits.is_empty() {
            continue;
        }
        let commit =
            crate::operations::update::commit_from_git_public(repo, &git_commit, ctx, remote_url)?;
        for path in hits {
            found.insert(path.to_string_lossy().replace('\\', "/"), commit.clone());
        }
    }
    Ok(found)
}

/// Blob id and mode of `path` in `tree`, or `None` when absent. A commit
/// touches a path exactly when this differs from its first parent's, which
/// is what a tree diff without rename detection reports.
fn entry_signature(tree: &git2::Tree<'_>, path: &Path) -> Option<(git2::Oid, i32)> {
    tree.get_path(path)
        .ok()
        .map(|entry| (entry.id(), entry.filemode()))
}

/// Populate `branches.local` and `branches.remote` for a commit that has a
/// `sha`. No-op for uncommitted-changes records.
///
/// A SHA pulled from the store may belong to a commit that lives only on
/// another clone — it's expected to be missing from this clone's object
/// database. We treat that as "not in any local/remote branch here" rather
/// than an error.
///
/// A store record whose commit has since reached a remote branch is demoted
/// to a plain remote commit. The issuing clone only rewrites its records on
/// its next update, so the record lingers after a push.
fn enrich_branches(repo: &Repository, mut commit: Commit) -> Result<Commit> {
    let Some(sha) = commit.sha.clone() else {
        return Ok(commit);
    };
    if commit.user.is_some() && repo.is_remote_commit(&sha).unwrap_or(false) {
        commit.host = None;
        commit.user = None;
        commit.clone = None;
        commit.branches = Branches::default();
    }
    if commit.branches.local.is_empty() {
        commit.branches.local = repo.local_branches_containing(&sha).unwrap_or_default();
    }
    if commit.branches.remote.is_empty() {
        commit.branches.remote = repo.remote_branches_containing(&sha).unwrap_or_default();
    }
    Ok(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use crate::spread::CommitSpread;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

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

    fn record(date: &str, sha: Option<&str>, remote: &str, changes: &[&str]) -> Commit {
        Commit {
            sha: sha.map(str::to_string),
            date: Some(date.to_string()),
            remote: Some(remote.to_string()),
            changes: changes.iter().map(|s| s.to_string()).collect(),
            ..Commit::default()
        }
    }

    #[test]
    fn store_index_picks_latest_record_per_path() {
        let commits = vec![
            record(
                "2026-01-01 00:00:00+00:00",
                Some("old"),
                "r",
                &["a.txt", "b.txt"],
            ),
            record("2026-01-02 00:00:00+00:00", Some("new"), "r", &["a.txt"]),
        ];
        let index = StoreIndex::new(&commits, "r", true);
        assert_eq!(
            index.latest("a.txt").and_then(|c| c.sha.as_deref()),
            Some("new")
        );
        assert_eq!(
            index.latest("b.txt").and_then(|c| c.sha.as_deref()),
            Some("old")
        );
        assert!(index.latest("c.txt").is_none());
    }

    #[test]
    fn store_index_skips_other_remotes_and_untracked_uncommitted() {
        let commits = vec![
            record(
                "2026-01-03 00:00:00+00:00",
                Some("elsewhere"),
                "other",
                &["a.txt"],
            ),
            record("2026-01-02 00:00:00+00:00", None, "r", &["a.txt"]),
            record(
                "2026-01-01 00:00:00+00:00",
                Some("committed"),
                "r",
                &["a.txt"],
            ),
        ];
        let tracked = StoreIndex::new(&commits, "r", true);
        assert!(tracked.latest("a.txt").unwrap().sha.is_none());
        let untracked = StoreIndex::new(&commits, "r", false);
        assert_eq!(
            untracked.latest("a.txt").and_then(|c| c.sha.as_deref()),
            Some("committed")
        );
    }

    #[test]
    fn unknown_file_yields_empty_commit() {
        let (_s, _o, m) = fixture(false);
        let repo = Repository::open(m.path()).unwrap();
        let result = last_commits(&repo, &["does/not/exist.txt".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename, "does/not/exist.txt");
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
        assert!(commit.branches.remote.iter().any(|b| b == "main"));
    }

    #[test]
    fn files_from_different_commits_resolve_in_one_pass() {
        let (_s, _o, m) = fixture(false);
        std::fs::write(m.path().join("second.txt"), b"two").unwrap();
        run(m.path(), &["add", "second.txt"]);
        run(m.path(), &["commit", "-m", "second"]);

        let repo = Repository::open(m.path()).unwrap();
        let files = ["README", "second.txt", "missing.txt", "README"].map(String::from);
        let result = last_commits(&repo, &files).unwrap();
        assert_eq!(result.len(), 4);
        assert!(result[0].commit.changes.iter().any(|p| p == "README"));
        assert!(result[1].commit.changes.iter().any(|p| p == "second.txt"));
        assert_ne!(result[0].commit.sha, result[1].commit.sha);
        assert!(result[2].commit.sha.is_none());
        assert_eq!(result[3].commit.sha, result[0].commit.sha);
    }

    #[test]
    fn local_only_commit_is_picked_up_from_store() {
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
    fn pushed_store_record_resolves_as_remote_commit() {
        let (_s, _o, m) = fixture(false);
        std::fs::write(m.path().join("draft.txt"), b"draft").unwrap();
        run(m.path(), &["add", "draft.txt"]);
        run(m.path(), &["commit", "-m", "wip"]);

        let repo = Repository::open(m.path()).unwrap();
        crate::operations::update_tracked_commits(&repo, &[]).unwrap();
        run(m.path(), &["push"]);

        let result = last_commits(&repo, &["draft.txt".to_string()]).unwrap();
        let c = &result[0].commit;
        assert!(
            c.user.is_none(),
            "pushed record should read as a remote commit"
        );
        assert!(c.branches.remote.iter().any(|b| b == "main"));
        assert_eq!(
            c.spread(Some("main"), &repo.context()),
            CommitSpread::MINE_ACTIVE_BRANCH | CommitSpread::REMOTE_MATCHING_BRANCH
        );
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
