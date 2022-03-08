class GitalongError(Exception):

    """Base error for gitalong."""


class GitalongNotInstalled(GitalongError):

    """Error for when gitalong is not installed in the managed repository."""
