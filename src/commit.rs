//! Tracked commit records.
//!
//! A [`Commit`] mirrors the Python `Commit(dict)` shape. JSON field names and
//! omission semantics match Python's `json.dump(...)` output so a
//! `commits.json` written by either implementation is readable by the other.
//!
//! The two flavors of commit:
//!
//! - **Real commits** — carry a `sha` and `author`, populated from the git
//!   log of the managed repository.
//! - **Uncommitted-changes commits** — carry no `sha`, but a `user` (the OS
//!   username of the clone that has the working-tree edits). Identified by
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

    /// OS username — present only on uncommitted-changes commits to identify
    /// the clone that issued them.
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
    /// real commit. Mirrors the Python `is_uncommitted_changes_commit` predicate.
    pub fn is_uncommitted_changes(&self) -> bool {
        self.user.is_some()
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
    /// the given clone — only uncommitted-changes commits ever satisfy this,
    /// since real commits don't carry `host`/`user`.
    ///
    /// Used during [`Self::spread`] to distinguish `MINE_UNCOMMITTED` from
    /// `THEIR_UNCOMMITTED`. The looser [`Self::is_ours`] is preferred for
    /// store filtering.
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
    /// Mirrors the Python `Commit.commit_spread` logic verbatim:
    ///
    /// - For uncommitted-changes commits, branches don't apply: light up
    ///   one of `MINE_UNCOMMITTED` / `THEIR_UNCOMMITTED` based on issuer.
    /// - For real commits with a known active branch on the local side,
    ///   light up `MINE_ACTIVE_BRANCH` / `THEIR_MATCHING_BRANCH` for the
    ///   matching-branch case and the equivalent `*_OTHER_BRANCH` flag
    ///   otherwise. Issuance is determined by whether the local branches
    ///   list contains the active branch (Python checks issuance via the
    ///   context dict in this branch — both readings are equivalent given
    ///   how local branches are populated only for the issuing clone).
    pub fn spread(&self, active_branch: Option<&str>, ctx: &Context) -> CommitSpread {
        let mut spread = CommitSpread::empty();

        if self.user.is_some() {
            // Uncommitted-changes commit, with or without an associated SHA.
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
            // Real commit: branch membership lights things up.
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
        // No nulls or empty objects on the wire.
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
        // Python serializes Commit dicts with all the same keys; gitdb-derived
        // fields shouldn't break us.
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
    fn is_uncommitted_changes_keys_off_user() {
        let real = Commit {
            sha: Some("abc".into()),
            author: Some("A".into()),
            ..Commit::default()
        };
        let uncommitted = Commit {
            user: Some("alice".into()),
            host: Some("host-1".into()),
            clone: Some("/work/repo".into()),
            ..Commit::default()
        };
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
        // No sha → pure uncommitted record.
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
        // Active branch is "main"; remote branches list includes both
        // "main" and "origin/main" — only the literal match counts as
        // REMOTE_MATCHING_BRANCH, the rest fall under REMOTE_OTHER_BRANCH.
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
