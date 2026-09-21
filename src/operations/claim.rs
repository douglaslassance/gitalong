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
    use crate::for_each_store;
    use crate::testing::{Team, git};
    use tempfile::TempDir;

    /// Alice with `shared.txt` committed and pushed, and Bob cloned from that.
    fn two_clones(team: &Team) -> (TempDir, TempDir) {
        let alice = team.seeded_clone("Alice", &[("shared.txt", "shared")], |c| {
            c.track_uncommitted = true
        });
        let bob = team.clone("Bob", |c| c.track_uncommitted = true);
        (alice, bob)
    }

    for_each_store!(claim_on_clean_file_is_unblocked, |kind| {
        let team = Team::new(kind);
        let (alice, _bob) = two_clones(&team);
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
    });

    for_each_store!(
        claim_blocked_when_other_clone_has_uncommitted_changes,
        |kind| {
            let team = Team::new(kind);
            let (alice, bob) = two_clones(&team);

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
    );

    for_each_store!(claim_blocked_when_other_clone_has_unpushed_commit, |kind| {
        let team = Team::new(kind);
        let (alice, bob) = two_clones(&team);
        std::fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
        git(alice.path(), &["commit", "-am", "edit shared"]);
        let alice_repo = Repository::open(alice.path()).unwrap();
        crate::operations::update_tracked_commits(&alice_repo, &[]).unwrap();

        let bob_repo = Repository::open(bob.path()).unwrap();
        let outcomes = claim_files(&bob_repo, &["shared.txt".to_string()]).unwrap();
        let blocker = &outcomes[0].blocker;
        assert!(blocker.sha.is_some(), "blocker should be Alice's commit");
        assert_eq!(blocker.author.as_deref(), Some("Alice"));
    });

    for_each_store!(claim_unblocked_once_other_clones_commit_is_pushed, |kind| {
        let team = Team::new(kind);
        let (alice, bob) = two_clones(&team);
        std::fs::write(alice.path().join("shared.txt"), b"edited").unwrap();
        git(alice.path(), &["commit", "-am", "edit shared"]);
        let alice_repo = Repository::open(alice.path()).unwrap();
        crate::operations::update_tracked_commits(&alice_repo, &[]).unwrap();
        git(alice.path(), &["push"]);
        git(bob.path(), &["pull", "--ff-only"]);

        let bob_repo = Repository::open(bob.path()).unwrap();
        let outcomes = claim_files(&bob_repo, &["shared.txt".to_string()]).unwrap();
        assert!(
            outcomes[0].blocker.sha.is_none(),
            "pushed work must not block a claim"
        );
    });
}
