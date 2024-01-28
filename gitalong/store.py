import typing

from abc import ABC, abstractmethod


class Store(ABC):
    """Abstract class for storing commits information."""

    def __init__(self, managed_repository):
        super().__init__()
        self._managed_repository = managed_repository

    @property
    @abstractmethod
    def commits(self) -> typing.List[dict]:
        """
        Returns:
            The stored commits.
        """
        return []

    @commits.setter
    @abstractmethod
    def commits(self, commits: typing.List[dict]):
        """Update the stored commits.

        Args: commits (list, optional): The commits to update the store with.
        """
        pass
