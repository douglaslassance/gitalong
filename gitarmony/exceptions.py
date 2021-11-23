class GitarmonyError(Exception):

    """Base error for gitarmony."""


class GitarmonyNotInstalled(GitarmonyError):

    """Error for when gitarmony is not installed in the managed repository."""
