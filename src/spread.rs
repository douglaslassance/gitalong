//! [`CommitSpread`]: bitfield describing where a commit lives.
//!
//! Each bit corresponds to one of eight scopes (this clone vs. theirs, the
//! active branch vs. another, committed vs. uncommitted). The CLI renders
//! the eight bits as a `+`/`-` string in the order the flags are declared
//! below; preserving that order is part of the wire contract.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Where a commit appears across branches and clones.
    #[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct CommitSpread: u8 {
        /// This clone's uncommitted changes.
        const MINE_UNCOMMITTED       = 1 << 0;
        /// This clone's currently checked-out branch.
        const MINE_ACTIVE_BRANCH     = 1 << 1;
        /// One of this clone's other local branches.
        const MINE_OTHER_BRANCH      = 1 << 2;
        /// The remote branch matching this clone's active branch name.
        const REMOTE_MATCHING_BRANCH = 1 << 3;
        /// A different remote branch.
        const REMOTE_OTHER_BRANCH    = 1 << 4;
        /// Another clone's non-matching branch.
        const THEIR_OTHER_BRANCH     = 1 << 5;
        /// Another clone's branch with the same name as this clone's active branch.
        const THEIR_MATCHING_BRANCH  = 1 << 6;
        /// Another clone's uncommitted changes.
        const THEIR_UNCOMMITTED      = 1 << 7;
    }
}

impl CommitSpread {
    /// Render the spread as the eight-character `+`/`-` string used by
    /// `gitalong status` and `gitalong claim`. Bit order matches the Python
    /// 0.x output and must not change without a CLI version bump.
    pub fn to_status_string(self) -> String {
        const ORDERED: [CommitSpread; 8] = [
            CommitSpread::MINE_UNCOMMITTED,
            CommitSpread::MINE_ACTIVE_BRANCH,
            CommitSpread::MINE_OTHER_BRANCH,
            CommitSpread::REMOTE_MATCHING_BRANCH,
            CommitSpread::REMOTE_OTHER_BRANCH,
            CommitSpread::THEIR_OTHER_BRANCH,
            CommitSpread::THEIR_MATCHING_BRANCH,
            CommitSpread::THEIR_UNCOMMITTED,
        ];
        ORDERED
            .iter()
            .map(|f| if self.contains(*f) { '+' } else { '-' })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_spread_is_all_minus() {
        assert_eq!(CommitSpread::empty().to_status_string(), "--------");
    }

    #[test]
    fn full_spread_is_all_plus() {
        assert_eq!(CommitSpread::all().to_status_string(), "++++++++");
    }

    #[test]
    fn first_bit_lights_first_position() {
        assert_eq!(
            CommitSpread::MINE_UNCOMMITTED.to_status_string(),
            "+-------"
        );
    }

    #[test]
    fn last_bit_lights_last_position() {
        assert_eq!(
            CommitSpread::THEIR_UNCOMMITTED.to_status_string(),
            "-------+"
        );
    }

    #[test]
    fn two_bits_render_independently() {
        let s = CommitSpread::MINE_ACTIVE_BRANCH | CommitSpread::REMOTE_MATCHING_BRANCH;
        assert_eq!(s.to_status_string(), "-+-+----");
    }
}
