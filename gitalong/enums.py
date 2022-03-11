from enum import IntFlag, auto


class CommitSpread(IntFlag):

    """A combinable enumerator to represent where the commit spreads across branches
    and clones.

    Attributes:
        LOCAL_UNCOMITTED (int): Commit represent our local uncommitted changes.
        LOCAL_ACTIVE_BRANCH (int): Commit is on our local active branch.
        LOCAL_OTHER_BRANCH (int): Commit is in one ore more of our other local branches.
        REMOTE_MATCHING_BRANCH (int): Commit is on matching remote branch.
        REMOTE_OTHER_BRANCH (int): Commit is on other remote branch.
        CLONE_OTHER_BRANCH (int): Commit is on someone else's clone non-matching branch.
        CLONE_MATCHING_BRANCH (int): Commit is on someone else's clone matching branch.
        CLONE_UNCOMMITED (int): Commit is on someone else's clone uncommitted changes.
    """

    LOCAL_UNCOMMITTED = auto()
    LOCAL_ACTIVE_BRANCH = auto()
    LOCAL_OTHER_BRANCH = auto()
    REMOTE_MATCHING_BRANCH = auto()
    REMOTE_OTHER_BRANCH = auto()
    CLONE_OTHER_BRANCH = auto()
    CLONE_MATCHING_BRANCH = auto()
    CLONE_UNCOMMITTED = auto()