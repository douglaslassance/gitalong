//! `gitalong update` — recompute and push this clone's contribution to the store.
//!
//! Mirrors the Python `update_tracked_commits()`:
//!
//! 1. Read the store's current commits.
//! 2. Drop any commit issued by this clone for this remote — they're stale, we're
//!    about to replace them with the live local view.
//! 3. Build the local-only view: every commit that isn't on a remote branch yet,
//!    plus an uncommitted-changes pseudo-commit when `track_uncommitted` is on.
//! 4. Concatenate and write back to the store.
//!
//! File-permission management (the Python `update_files_permissions`) is part
//! of a later phase and intentionally not invoked here.

use time::OffsetDateTime;
use time::UtcOffset;
use time::macros::format_description;

use crate::commit::{Branches, Commit};
use crate::error::{Error, Result};
use crate::repository::{Context, Repository};
use crate::store::Store;

/// Refresh the store with this clone's local view.
///
/// `claims` are extra repo-relative paths to attach to the uncommitted-changes
/// pseudo-commit (used by `gitalong claim` to mark files as in-flight without
/// requiring the user to actually edit them yet).
pub fn update_tracked_commits(repo: &Repository, claims: &[String]) -> Result<()> {
    let mut store = Store::for_repository(repo)?;
    let existing = store.read()?;
    let context = repo.context();
    let remote_url = repo.remote_url()?.unwrap_or_default();

    // Step 1: drop our own stale entries for this remote.
    //
    // The Python filter used `is_issued_commit()` (full host/user/clone match),
    // which never matches real commits — they only carry `clone`. That left
    // every previously-tracked real commit in the store, doubling up on each
    // re-run. The looser `is_ours` (clone-path match) catches both real and
    // uncommitted records this clone contributed.
    let mut next: Vec<Commit> = existing
        .into_iter()
        .filter(|c| {
            let other_remote = c.remote.as_deref() != Some(remote_url.as_str());
            other_remote || !c.is_ours(&context)
        })
        .collect();

    // Step 2: append the fresh local-only view.
    next.extend(local_only_commits(repo, claims, &context, &remote_url)?);

    store.write(&next)?;

    // When the repo opted into permission management, refresh the write-bit
    // for every tracked file based on the freshly-written commit set.
    if repo.config().modify_permissions {
        let files = repo.tracked_files_at_head()?;
        crate::operations::update_files_permissions(repo, &files)?;
    }
    Ok(())
}

/// Walk the local-only commit graph plus the optional uncommitted-changes
/// pseudo-commit. Sort newest-first by date so the on-disk JSON is stable
/// across runs.
fn local_only_commits(
    repo: &Repository,
    claims: &[String],
    context: &Context,
    remote_url: &str,
) -> Result<Vec<Commit>> {
    let mut commits: Vec<Commit> = real_local_only_commits(repo, context, remote_url)?;
    if repo.config().track_uncommitted
        && let Some(uc) = uncommitted_changes_commit(repo, claims, context, remote_url)?
    {
        commits.insert(0, uc);
    }
    commits.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(commits)
}

/// Real commits reachable from any local branch but not from any remote branch.
///
/// Implemented with a single git2 revwalk (`refs/heads/*` minus
/// `refs/remotes/*`) which subsumes the Python recursion across branches. The
/// per-commit metadata (changes, branches.local) is computed from git2 too.
fn real_local_only_commits(
    repo: &Repository,
    context: &Context,
    remote_url: &str,
) -> Result<Vec<Commit>> {
    let inner = repo.git();
    let mut walk = inner.revwalk()?;
    walk.set_sorting(git2::Sort::TIME)?;

    // Push every local branch tip — `push_glob` happily matches across the
    // single segment under `refs/heads/`.
    walk.push_glob("refs/heads/*")?;

    // Hide remote-tracking branches explicitly. `hide_glob("refs/remotes/*")`
    // would seem natural but its `*` doesn't cross slashes, so it misses the
    // common `refs/remotes/<remote>/<branch>` layout entirely.
    for entry in inner.branches(Some(git2::BranchType::Remote))? {
        let (branch, _) = entry?;
        if let Some(oid) = branch.get().target() {
            walk.hide(oid)?;
        }
    }

    let mut out = Vec::new();
    for oid in walk {
        let oid = oid?;
        let git_commit = inner.find_commit(oid)?;
        out.push(commit_from_git(
            repo, &git_commit, context, remote_url,
        )?);
    }
    Ok(out)
}

/// Build a [`Commit`] record from a `git2::Commit`, populating the fields the
/// Python tool expects. Re-exported for the status pipeline as well.
pub(crate) fn commit_from_git_public(
    repo: &Repository,
    git_commit: &git2::Commit<'_>,
    context: &Context,
    remote_url: &str,
) -> Result<Commit> {
    commit_from_git(repo, git_commit, context, remote_url)
}

fn commit_from_git(
    repo: &Repository,
    git_commit: &git2::Commit<'_>,
    context: &Context,
    remote_url: &str,
) -> Result<Commit> {
    let sha = git_commit.id().to_string();

    let date = format_git_time(git_commit.time())?;
    let author = git_commit
        .author()
        .name()
        .map(str::to_string)
        .unwrap_or_default();

    let changes = changed_files(repo, git_commit)?;
    let local = repo.local_branches_containing(&sha)?;

    Ok(Commit {
        sha: Some(sha),
        author: Some(author),
        date: Some(date),
        remote: Some(remote_url.to_string()),
        clone: Some(context.clone.to_string_lossy().into_owned()),
        host: Some(context.host.clone()),
        user: None,
        changes,
        branches: Branches {
            local,
            remote: Vec::new(),
        },
    })
}

/// Repo-relative paths affected by a commit, derived from a tree diff against
/// its first parent (or the empty tree for root commits).
fn changed_files(repo: &Repository, c: &git2::Commit<'_>) -> Result<Vec<String>> {
    let inner = repo.git();
    let new_tree = c.tree()?;
    let old_tree = if c.parent_count() == 0 {
        None
    } else {
        Some(c.parent(0)?.tree()?)
    };

    let diff = inner.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)?;

    let mut files = std::collections::BTreeSet::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path())
                && let Some(s) = p.to_str()
            {
                files.insert(s.replace('\\', "/"));
            }
            true
        },
        None,
        None,
        None,
    )?;
    Ok(files.into_iter().collect())
}

/// Build the uncommitted-changes pseudo-commit, or `None` when there's
/// nothing dirty *and* no claims to record.
fn uncommitted_changes_commit(
    repo: &Repository,
    claims: &[String],
    context: &Context,
    remote_url: &str,
) -> Result<Option<Commit>> {
    let mut paths = repo.uncommitted_change_paths()?;
    for claim in claims {
        if !paths.contains(claim) {
            paths.push(claim.clone());
        }
    }
    if paths.is_empty() {
        return Ok(None);
    }
    paths.sort();

    // `time` only exposes UTC `now` without the optional `local-offset` feature.
    // The Python tool stamps with the local clock, but UTC sorts the same way
    // and is unambiguous across timezones — a small, intentional deviation.
    let date = format_offset_date_time(OffsetDateTime::now_utc())?;

    let mut c = Commit {
        date: Some(date),
        remote: Some(remote_url.to_string()),
        changes: paths,
        ..Commit::default()
    };
    c.stamp_context(context);
    Ok(Some(c))
}

/// Format a `git2::Time` as `YYYY-MM-DD HH:MM:SS+HH:MM` to match the Python
/// `str(datetime)` output the on-disk `commits.json` is sorted by.
fn format_git_time(t: git2::Time) -> Result<String> {
    let offset = UtcOffset::from_whole_seconds(t.offset_minutes() * 60)
        .unwrap_or(UtcOffset::UTC);
    let dt = OffsetDateTime::from_unix_timestamp(t.seconds())
        .map_err(|e| Error::InvalidConfig(format!("git commit timestamp: {e}")))?
        .to_offset(offset);
    format_offset_date_time(dt)
}

fn format_offset_date_time(dt: OffsetDateTime) -> Result<String> {
    let fmt = format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
    );
    dt.format(&fmt)
        .map_err(|e| Error::InvalidConfig(format!("date format: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    /// Stand up a managed clone + bare store + remote-of-managed so revwalk
    /// has something realistic to walk.
    struct Fixture {
        _store: TempDir,
        _origin: TempDir,
        managed: TempDir,
    }

    fn run(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn make_fixture() -> Fixture {
        // Bare store repo (gitalong's commits.json target).
        let store = tempfile::tempdir().unwrap();
        run(store.path(), &["init", "--bare", "--initial-branch=main"]);

        // Bare "origin" repo for the managed code.
        let origin = tempfile::tempdir().unwrap();
        run(origin.path(), &["init", "--bare", "--initial-branch=main"]);

        // Managed repo: cloned from origin. We seed README *and* the gitalong
        // config in the same initial commit so the working tree is pristine
        // afterwards — leaving .gitalong.json as an untracked file would make
        // every test see a stray uncommitted-changes record.
        let managed = tempfile::tempdir().unwrap();
        run(
            managed.path(),
            &[
                "clone",
                origin.path().to_str().unwrap(),
                managed.path().to_str().unwrap(),
            ],
        );
        run(managed.path(), &["config", "user.email", "alice@example.com"]);
        run(managed.path(), &["config", "user.name", "Alice"]);

        let cfg = Config {
            store_url: format!("file://{}", store.path().display()),
            pull_threshold: 0.0,
            track_uncommitted: true,
            ..Config::default()
        };
        cfg.save(&managed.path().join(CONFIG_BASENAME)).unwrap();
        std::fs::write(managed.path().join("README"), b"hi").unwrap();
        // The cloned store lives in `<managed>/.gitalong/`. Without ignoring
        // it the post-clone working tree would look dirty to status, polluting
        // every uncommitted-changes record under track_uncommitted=true.
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

        Fixture {
            _store: store,
            _origin: origin,
            managed,
        }
    }

    #[test]
    fn nothing_to_track_after_pristine_setup() {
        let f = make_fixture();
        let repo = Repository::open(f.managed.path()).unwrap();
        update_tracked_commits(&repo, &[]).unwrap();

        let mut store = Store::for_repository(&repo).unwrap();
        // Everything is on the remote and there are no uncommitted edits, so
        // the store should land empty.
        assert!(store.read().unwrap().is_empty());
    }

    #[test]
    fn local_only_commit_makes_it_into_the_store() {
        let f = make_fixture();
        // A commit that hasn't been pushed yet — exactly the case gitalong
        // needs to surface.
        std::fs::write(f.managed.path().join("local.txt"), b"local-only").unwrap();
        run(f.managed.path(), &["add", "local.txt"]);
        run(f.managed.path(), &["commit", "-m", "wip"]);

        let repo = Repository::open(f.managed.path()).unwrap();
        update_tracked_commits(&repo, &[]).unwrap();

        let mut store = Store::for_repository(&repo).unwrap();
        let commits = store.read().unwrap();
        assert_eq!(commits.len(), 1, "expected the one local-only commit");
        let c = &commits[0];
        assert!(c.sha.is_some());
        assert_eq!(c.author.as_deref(), Some("Alice"));
        assert_eq!(c.changes, vec!["local.txt".to_string()]);
        assert_eq!(c.branches.local, vec!["main".to_string()]);
        // The host stamp ties the commit to this clone, distinguishing it
        // from another team member's commit on the same branch.
        assert!(c.host.is_some());
    }

    #[test]
    fn uncommitted_changes_appear_when_track_uncommitted() {
        let f = make_fixture();
        std::fs::write(f.managed.path().join("draft.txt"), b"in flight").unwrap();
        // Don't add or commit — purely uncommitted.

        let repo = Repository::open(f.managed.path()).unwrap();
        update_tracked_commits(&repo, &[]).unwrap();

        let mut store = Store::for_repository(&repo).unwrap();
        let commits = store.read().unwrap();
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert!(c.sha.is_none(), "uncommitted commit must not carry a sha");
        assert!(c.is_uncommitted_changes());
        assert!(c.changes.iter().any(|p| p == "draft.txt"));
    }

    #[test]
    fn rerun_replaces_our_previous_local_view() {
        let f = make_fixture();
        // First wave: one local commit.
        std::fs::write(f.managed.path().join("a.txt"), b"first").unwrap();
        run(f.managed.path(), &["add", "a.txt"]);
        run(f.managed.path(), &["commit", "-m", "first"]);

        let repo = Repository::open(f.managed.path()).unwrap();
        update_tracked_commits(&repo, &[]).unwrap();

        // Second wave: a second local commit, then rerun. The store must
        // reflect the two-commit local view, not the one-commit + carry-over.
        std::fs::write(f.managed.path().join("b.txt"), b"second").unwrap();
        run(f.managed.path(), &["add", "b.txt"]);
        run(f.managed.path(), &["commit", "-m", "second"]);
        update_tracked_commits(&repo, &[]).unwrap();

        let mut store = Store::for_repository(&repo).unwrap();
        let commits = store.read().unwrap();
        assert_eq!(
            commits.len(),
            2,
            "rerun must replace the previous local view, not append to it"
        );
        // Sorted newest-first by date — the "second" commit should lead.
        assert!(commits[0].changes.iter().any(|c| c == "b.txt"));
    }

    #[test]
    fn claim_paths_are_added_to_uncommitted_record() {
        let f = make_fixture();
        let repo = Repository::open(f.managed.path()).unwrap();
        // No actual edits: only an explicit claim. With track_uncommitted on,
        // the claim alone should produce an uncommitted-changes record.
        update_tracked_commits(&repo, &["claimed.txt".to_string()]).unwrap();

        let mut store = Store::for_repository(&repo).unwrap();
        let commits = store.read().unwrap();
        assert_eq!(commits.len(), 1);
        assert!(commits[0].is_uncommitted_changes());
        assert!(commits[0].changes.iter().any(|c| c == "claimed.txt"));
    }
}
