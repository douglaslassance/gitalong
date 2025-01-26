class RepositoryNotSetup(Exception):
    """Error for when gitalong is not setup in the managed repository."""


class RepositoryInvalidConfig(Exception):
    """Error for when gitalong config is not as expected."""


class StoreNotReachable(Exception):
    """Error for when the store is not reachable."""


class CommandError(Exception):
    """Error for when running a command."""
