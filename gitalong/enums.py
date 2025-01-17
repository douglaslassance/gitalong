from enum import IntFlag, auto


class CommitSpread(IntFlag):
    """A combinable enumerator to represent where the commit spreads across branches
    and clones.

    Attributes:
        MINE_UNCOMITTED (int): Commit represent our local uncommitted changes.
        MINE_CLAIMED (int): The commit represent our claimed files.
        MINE_ACTIVE_BRANCH (int): Commit is on our local active branch.
        MINE_OTHER_BRANCH (int): Commit is in one ore more of our other local branches.
        REMOTE_MATCHING_BRANCH (int): Commit is on matching remote branch.
        REMOTE_OTHER_BRANCH (int): Commit is on other remote branch.
        THEIR_OTHER_BRANCH (int): Commit is on someone else's clone non-matching branch.
        THEIR_MATCHING_BRANCH (int): Commit is on someone else's clone matching branch.
        THEIR_CLAIMED (int): Commit represent someone else's claimed files.
        THEIR_UNCOMMITED (int): Commit is on someone else's clone uncommitted changes.
    """

    MINE_UNCOMMITTED = auto()
    MINE_CLAIMED = auto()
    MINE_ACTIVE_BRANCH = auto()
    MINE_OTHER_BRANCH = auto()
    REMOTE_MATCHING_BRANCH = auto()
    REMOTE_OTHER_BRANCH = auto()
    THEIR_OTHER_BRANCH = auto()
    THEIR_MATCHING_BRANCH = auto()
    THEIR_CLAIMED = auto()
    THEIR_UNCOMMITTED = auto()
