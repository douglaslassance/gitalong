//! `gitalong claim` — try to mark files as in-flight for this clone.
//!
//! For each requested path, the workflow is:
//!
//! 1. Look up the current latest commit for the file via [`last_commits`].
//! 2. Decide whether this clone is allowed to claim it. Following the Python
//!    rule, only files whose spread is *exactly* this clone's active branch
//!    or this clone's uncommitted state qualify; anything else means another
//!    clone or branch holds the file and the claim is blocked.
//! 3. Run [`update_tracked_commits`] with the unblocked files as `claims` so
//!    they show up as uncommitted changes for this clone.
//!
//! The function returns one [`ClaimOutcome`] per input path so the CLI can
//! print a status line for each.

use crate::commit::Commit;
use crate::error::Result;
use crate::operations::status::{FileStatus, last_commits};
use crate::operations::update::update_tracked_commits;
use crate::repository::Repository;
use crate::spread::CommitSpread;

/// Per-file result of a claim attempt.
pub struct ClaimOutcome {
    /// File the user asked about, preserved verbatim for output.
    pub filename: String,
    /// The blocking commit when the claim is denied. Empty default-`Commit`
    /// when the claim is allowed — printed as all-dashes by `format_status`.
    pub blocker: Commit,
}

/// Attempt to claim each path in `files` for this clone.
pub fn claim_files(repo: &Repository, files: &[String]) -> Result<Vec<ClaimOutcome>> {
    let active = repo.active_branch_name()?;
    let ctx = repo.context();

    let statuses: Vec<FileStatus> = last_commits(repo, files)?;
    let mut outcomes = Vec::with_capacity(statuses.len());
    let mut allowed_claims = Vec::new();

    for status in statuses {
        let spread = status.commit.spread(active.as_deref(), &ctx);
        let already_ours = spread.intersects(CommitSpread::MINE_ACTIVE_BRANCH)
            || spread.intersects(CommitSpread::MINE_UNCOMMITTED);
        let unblocked = already_ours || spread.is_empty();

        if unblocked {
            allowed_claims.push(repo_relative(&status.filename, repo));
            outcomes.push(ClaimOutcome {
                filename: status.filename,
                blocker: Commit::default(),
            });
        } else {
            outcomes.push(ClaimOutcome {
                filename: status.filename,
                blocker: status.commit,
            });
        }
    }

    if !allowed_claims.is_empty() {
        update_tracked_commits(repo, &allowed_claims)?;
    }
    Ok(outcomes)
}

fn repo_relative(filename: &str, repo: &Repository) -> String {
    let abs = repo.absolute_path(std::path::Path::new(filename));
    repo.relative_path(&abs)
        .to_string_lossy()
        .replace('\\', "/")
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
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Two managed clones sharing the same store, set up so we can simulate
    /// a contested-claim scenario.
    fn two_clones() -> (TempDir, TempDir, TempDir, TempDir) {
        let store = tempfile::tempdir().unwrap();
        run(store.path(), &["init", "--bare", "--initial-branch=main"]);
        let origin = tempfile::tempdir().unwrap();
        run(origin.path(), &["init", "--bare", "--initial-branch=main"]);

        let alice = tempfile::tempdir().unwrap();
        run(
            alice.path(),
            &[
                "clone",
                origin.path().to_str().unwrap(),
                alice.path().to_str().unwrap(),
            ],
        );
        run(alice.path(), &["config", "user.email", "alice@example.com"]);
        run(alice.path(), &["config", "user.name", "Alice"]);
        let cfg = Config {
            store_url: format!("file://{}", store.path().display()),
            pull_threshold: 0.0,
            track_uncommitted: true,
            ..Config::default()
        };
        cfg.save(&alice.path().join(CONFIG_BASENAME)).unwrap();
        std::fs::write(
            alice.path().join(".gitignore"),
            crate::hooks::GITIGNORE_PATCH,
        )
        .unwrap();
        std::fs::write(alice.path().join("shared.txt"), b"shared").unwrap();
        run(
            alice.path(),
            &["add", "shared.txt", ".gitalong.json", ".gitignore"],
        );
        run(alice.path(), &["commit", "-m", "init"]);
        run(alice.path(), &["push", "-u", "origin", "main"]);

        let bob = tempfile::tempdir().unwrap();
        run(
            bob.path(),
            &[
                "clone",
                origin.path().to_str().unwrap(),
                bob.path().to_str().unwrap(),
            ],
        );
        run(bob.path(), &["config", "user.email", "bob@example.com"]);
        run(bob.path(), &["config", "user.name", "Bob"]);

        (store, origin, alice, bob)
    }

    #[test]
    fn claim_on_clean_file_is_unblocked() {
        let (_s, _o, alice, _bob) = two_clones();
        let repo = Repository::open(alice.path()).unwrap();

        let outcomes = claim_files(&repo, &["shared.txt".to_string()]).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].blocker.sha.is_none());
        assert!(outcomes[0].blocker.host.is_none());

        let mut store = crate::store::Store::for_repository(&repo).unwrap();
        let commits = store.read().unwrap();
        let ours = commits
            .iter()
            .find(|c| c.is_uncommitted_changes())
            .expect("expected an uncommitted-changes record");
        assert!(ours.changes.iter().any(|p| p == "shared.txt"));
    }

    #[test]
    fn claim_blocked_when_other_clone_has_uncommitted_changes() {
        let (_s, _o, alice, bob) = two_clones();

        let alice_repo = Repository::open(alice.path()).unwrap();
        claim_files(&alice_repo, &["shared.txt".to_string()]).unwrap();

        let bob_repo = Repository::open(bob.path()).unwrap();
        let outcomes = claim_files(&bob_repo, &["shared.txt".to_string()]).unwrap();
        let blocker = &outcomes[0].blocker;
        assert!(
            blocker.is_uncommitted_changes(),
            "blocker should be an uncommitted-changes record"
        );
        let bob_clone = bob_repo.context().clone.to_string_lossy().into_owned();
        assert_ne!(
            blocker.clone.as_deref(),
            Some(bob_clone.as_str()),
            "blocker should not be Bob's own record"
        );
    }

    #[test]
    fn claim_blocked_when_other_clone_has_unpushed_commit() {
        let (_s, _o, alice, bob) = two_clones();
        std::fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
        run(alice.path(), &["commit", "-am", "edit shared"]);
        let alice_repo = Repository::open(alice.path()).unwrap();
        crate::operations::update_tracked_commits(&alice_repo, &[]).unwrap();

        let bob_repo = Repository::open(bob.path()).unwrap();
        let outcomes = claim_files(&bob_repo, &["shared.txt".to_string()]).unwrap();
        let blocker = &outcomes[0].blocker;
        assert!(blocker.sha.is_some(), "blocker should be Alice's commit");
        assert_eq!(blocker.author.as_deref(), Some("Alice"));
    }

    #[test]
    fn claim_unblocked_once_other_clones_commit_is_pushed() {
        let (_s, _o, alice, bob) = two_clones();
        std::fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
        run(alice.path(), &["commit", "-am", "edit shared"]);
        let alice_repo = Repository::open(alice.path()).unwrap();
        crate::operations::update_tracked_commits(&alice_repo, &[]).unwrap();
        run(alice.path(), &["push"]);
        run(bob.path(), &["pull", "--ff-only"]);

        let bob_repo = Repository::open(bob.path()).unwrap();
        let outcomes = claim_files(&bob_repo, &["shared.txt".to_string()]).unwrap();
        assert!(
            outcomes[0].blocker.sha.is_none(),
            "pushed work must not block a claim"
        );
    }
}
