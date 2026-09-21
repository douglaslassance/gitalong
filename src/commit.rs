//! Tracked commit records.
//!
//! A [`Commit`] mirrors the Python `Commit(dict)` shape. JSON field names and
//! omission semantics match Python's `json.dump(...)` output so a
//! `commits.json` written by either implementation is readable by the other.
//!
//! The two flavors of commit:
//!
//! - **Real commits** — carry a `sha` and `author`, populated from the git
//!   log of the managed repository. Those written to the store also carry the
//!   issuing clone's `host`, `user` and `clone` so readers can tell whose
//!   unpushed work they are.
//! - **Uncommitted-changes commits** — carry no `sha`, only the issuing
//!   context and the dirty paths. Identified by
//!   [`Commit::is_uncommitted_changes`].

use serde::{Deserialize, Serialize};

use crate::repository::Context;
use crate::spread::CommitSpread;

/// A single tracked commit (real or uncommitted-changes pseudo-commit).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    /// Real commit SHA. Absent for uncommitted-changes commits.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sha: Option<String>,

    /// OS username of the issuing clone. Present on every store record,
    /// absent on commits read straight from the git log.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub user: Option<String>,

    /// Hostname of the issuing clone.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub host: Option<String>,

    /// Canonical working-tree path of the issuing clone.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub clone: Option<String>,

    /// Remote URL of the managed repository.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remote: Option<String>,

    /// Commit date — ISO-8601 string for real commits, `now()` for uncommitted ones.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub date: Option<String>,

    /// Author name (real commits only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub author: Option<String>,

    /// Files this commit modified, as repo-relative paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,

    /// Branch membership of this commit.
    #[serde(default, skip_serializing_if = "Branches::is_empty")]
    pub branches: Branches,
}

/// Local and remote branch lists a commit belongs to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Branches {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote: Vec<String>,
}

impl Branches {
    pub fn is_empty(&self) -> bool {
        self.local.is_empty() && self.remote.is_empty()
    }
}

impl Commit {
    /// `true` when this record represents uncommitted changes rather than a
    /// real commit.
    pub fn is_uncommitted_changes(&self) -> bool {
        self.sha.is_none()
    }

    /// `true` when this record was contributed by the given clone, identified
    /// by the canonicalized working-tree path stored in `clone`.
    ///
    /// Used by [`crate::operations::update_tracked_commits`] to drop our own
    /// previous entries before re-emitting the live local view. Both real
    /// commits (which carry `clone` via `git_commit.repo.working_dir`) and
    /// uncommitted-changes records (stamped via [`Self::stamp_context`]) are
    /// matched by this predicate.
    pub fn is_ours(&self, ctx: &Context) -> bool {
        let clone_str = ctx.clone.to_string_lossy();
        self.clone.as_deref() == Some(clone_str.as_ref())
    }

    /// `true` when this record carries the full host/user/clone context of
    /// the given clone.
    ///
    /// Used during [`Self::spread`] to tell `MINE_*` from `THEIR_*`. The
    /// looser [`Self::is_ours`] is preferred for store filtering.
    pub fn is_issued_by(&self, ctx: &Context) -> bool {
        let clone_str = ctx.clone.to_string_lossy();
        self.host.as_deref() == Some(ctx.host.as_str())
            && self.user.as_deref() == Some(ctx.user.as_str())
            && self.clone.as_deref() == Some(clone_str.as_ref())
    }

    /// Stamp this commit with the given identity context. Used when issuing
    /// uncommitted-changes commits or claims.
    pub fn stamp_context(&mut self, ctx: &Context) {
        self.host = Some(ctx.host.clone());
        self.user = Some(ctx.user.clone());
        self.clone = Some(ctx.clone.to_string_lossy().into_owned());
    }

    /// Compute the [`CommitSpread`] for this record relative to the given
    /// active branch and identity context.
    ///
    /// Mirrors the Python `Commit.commit_spread` logic:
    ///
    /// - Store records (those with a `user`) are someone's unpushed work.
    ///   Uncommitted ones light `MINE_UNCOMMITTED` / `THEIR_UNCOMMITTED`;
    ///   real ones light `MINE_ACTIVE_BRANCH` / `THEIR_MATCHING_BRANCH` when
    ///   the issuer's local branches include the active branch and the
    ///   `*_OTHER_BRANCH` flag otherwise. Mine versus theirs is decided by
    ///   [`Self::is_issued_by`].
    /// - Commits read from the git log (no `user`) are already public: light
    ///   the `REMOTE_*` flags from their remote branches, plus
    ///   `MINE_ACTIVE_BRANCH` when the active branch contains them.
    pub fn spread(&self, active_branch: Option<&str>, ctx: &Context) -> CommitSpread {
        let mut spread = CommitSpread::empty();

        if self.user.is_some() {
            let issued = self.is_issued_by(ctx);
            if self.sha.is_some() {
                let on_active = active_branch
                    .map(|ab| self.branches.local.iter().any(|b| b == ab))
                    .unwrap_or(false);
                spread |= if on_active {
                    if issued {
                        CommitSpread::MINE_ACTIVE_BRANCH
                    } else {
                        CommitSpread::THEIR_MATCHING_BRANCH
                    }
                } else if issued {
                    CommitSpread::MINE_OTHER_BRANCH
                } else {
                    CommitSpread::THEIR_OTHER_BRANCH
                };
            } else {
                spread |= if issued {
                    CommitSpread::MINE_UNCOMMITTED
                } else {
                    CommitSpread::THEIR_UNCOMMITTED
                };
            }
        } else {
            let mut remote_branches = self.branches.remote.clone();
            if let Some(active) = active_branch {
                if remote_branches.iter().any(|b| b == active) {
                    spread |= CommitSpread::REMOTE_MATCHING_BRANCH;
                    remote_branches.retain(|b| b != active);
                }
                if self.branches.local.iter().any(|b| b == active) {
                    spread |= CommitSpread::MINE_ACTIVE_BRANCH;
                }
            }
            if !remote_branches.is_empty() {
                spread |= CommitSpread::REMOTE_OTHER_BRANCH;
            }
        }

        spread
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> Context {
        Context {
            host: "host-1".into(),
            user: "alice".into(),
            clone: PathBuf::from("/work/repo"),
        }
    }

    #[test]
    fn real_commit_serialization_omits_uncommitted_fields() {
        let c = Commit {
            sha: Some("abc".into()),
            author: Some("Alice".into()),
            ..Commit::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("user"));
        assert!(!json.contains("host"));
        assert!(!json.contains("branches"));
        assert!(!json.contains("changes"));
    }

    #[test]
    fn round_trip_through_json() {
        let original = Commit {
            sha: Some("deadbeef".into()),
            author: Some("Bob".into()),
            date: Some("2026-05-08 12:00:00".into()),
            remote: Some("git@example.com:foo.git".into()),
            changes: vec!["a.txt".into(), "b.txt".into()],
            branches: Branches {
                local: vec!["main".into()],
                remote: vec!["origin/main".into()],
            },
            ..Commit::default()
        };
        let s = serde_json::to_string(&original).unwrap();
        let back: Commit = serde_json::from_str(&s).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn deserializes_python_dict_with_extras() {
        let raw = r#"{
            "sha": "abc",
            "author": "Alice",
            "date": "2026-05-08 12:00:00",
            "branches": {"local": ["main"], "remote": ["origin/main"]},
            "changes": ["a.txt"]
        }"#;
        let c: Commit = serde_json::from_str(raw).unwrap();
        assert_eq!(c.sha.as_deref(), Some("abc"));
        assert_eq!(c.changes, vec!["a.txt".to_string()]);
    }

    #[test]
    fn is_uncommitted_changes_keys_off_missing_sha() {
        let mut real = Commit {
            sha: Some("abc".into()),
            author: Some("A".into()),
            ..Commit::default()
        };
        real.stamp_context(&ctx());
        let mut uncommitted = Commit::default();
        uncommitted.stamp_context(&ctx());
        assert!(!real.is_uncommitted_changes());
        assert!(uncommitted.is_uncommitted_changes());
    }

    #[test]
    fn is_issued_by_matches_full_context() {
        let mut c = Commit::default();
        c.stamp_context(&ctx());
        assert!(c.is_issued_by(&ctx()));
    }

    #[test]
    fn is_issued_by_rejects_other_clone() {
        let mut c = Commit::default();
        c.stamp_context(&ctx());
        let other = Context {
            host: "host-2".into(),
            ..ctx()
        };
        assert!(!c.is_issued_by(&other));
    }

    #[test]
    fn spread_for_my_uncommitted() {
        let mut c = Commit::default();
        c.stamp_context(&ctx());
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::MINE_UNCOMMITTED
        );
    }

    #[test]
    fn spread_for_their_uncommitted() {
        let mut c = Commit::default();
        c.stamp_context(&Context {
            host: "host-2".into(),
            user: "bob".into(),
            clone: PathBuf::from("/elsewhere"),
        });
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::THEIR_UNCOMMITTED
        );
    }

    fn their_ctx() -> Context {
        Context {
            host: "host-2".into(),
            user: "bob".into(),
            clone: PathBuf::from("/elsewhere"),
        }
    }

    fn stored_real_commit(issuer: &Context, local: &[&str]) -> Commit {
        let mut c = Commit {
            sha: Some("abc".into()),
            author: Some("Someone".into()),
            branches: Branches {
                local: local.iter().map(|s| s.to_string()).collect(),
                remote: Vec::new(),
            },
            ..Commit::default()
        };
        c.stamp_context(issuer);
        c
    }

    #[test]
    fn spread_for_my_unpushed_commit_on_active_branch() {
        let c = stored_real_commit(&ctx(), &["main"]);
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::MINE_ACTIVE_BRANCH
        );
    }

    #[test]
    fn spread_for_my_unpushed_commit_on_other_branch() {
        let c = stored_real_commit(&ctx(), &["feature"]);
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::MINE_OTHER_BRANCH
        );
    }

    #[test]
    fn spread_for_their_unpushed_commit_on_matching_branch() {
        let c = stored_real_commit(&their_ctx(), &["main"]);
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::THEIR_MATCHING_BRANCH
        );
    }

    #[test]
    fn spread_for_their_unpushed_commit_on_other_branch() {
        let c = stored_real_commit(&their_ctx(), &["feature"]);
        assert_eq!(
            c.spread(Some("main"), &ctx()),
            CommitSpread::THEIR_OTHER_BRANCH
        );
    }

    #[test]
    fn spread_for_real_commit_on_active_local_and_remote() {
        let c = Commit {
            sha: Some("abc".into()),
            branches: Branches {
                local: vec!["main".into()],
                remote: vec!["origin/main".into(), "main".into()],
            },
            ..Commit::default()
        };
        let spread = c.spread(Some("main"), &ctx());
        assert!(spread.contains(CommitSpread::MINE_ACTIVE_BRANCH));
        assert!(spread.contains(CommitSpread::REMOTE_MATCHING_BRANCH));
        assert!(spread.contains(CommitSpread::REMOTE_OTHER_BRANCH));
    }

    #[test]
    fn spread_for_real_commit_only_on_remote_other() {
        let c = Commit {
            sha: Some("abc".into()),
            branches: Branches {
                local: vec![],
                remote: vec!["origin/feature".into()],
            },
            ..Commit::default()
        };
        let spread = c.spread(Some("main"), &ctx());
        assert_eq!(spread, CommitSpread::REMOTE_OTHER_BRANCH);
    }
}
