from enum import IntFlag, auto


class Spread(IntFlag):

    """A combinable enum to represent where the commit spreads across branches and
    clones.

    Attributes:
        CLONE_MATCHING_BRANCH (TYPE): Description
        CLONE_OTHER_BRANCH (TYPE): Description
        CLONE_UNCOMMITED (TYPE): Description
        LOCAL_ACTIVE_BRANCH (TYPE): Description
        LOCAL_OTHER_BRANCH (TYPE): Description
        LOCAL_UNCOMITTED (TYPE): Description
        REMOTE_MATCHING_BRANCH (TYPE): Description
        REMOTE_OTHER_BRANCH (TYPE): Description
    """

    LOCAL_UNCOMMITTED = auto()
    LOCAL_ACTIVE_BRANCH = auto()
    LOCAL_OTHER_BRANCH = auto()
    REMOTE_MATCHING_BRANCH = auto()
    REMOTE_OTHER_BRANCH = auto()
    CLONE_OTHER_BRANCH = auto()
    CLONE_MATCHING_BRANCH = auto()
    CLONE_UNCOMMITTED = auto()
