import json
import os
import typing
from abc import ABC, abstractmethod


class Store(ABC):
    """Abstract class for storing commits information."""

    def __init__(self, managed_repository):
        super().__init__()
        self._managed_repository = managed_repository
        self._local_json = os.path.join(
            self._managed_repository.working_dir, ".gitalong", "commits.json"
        )

    def _read_local_json(self) -> typing.List[dict]:
        if os.path.exists(self._local_json_path):
            with open(self._local_json_path, "r", encoding="utf-8") as fle:
                return json.loads(fle.read())
        return []

    def _write_local_json(self, commits: typing.List[dict]):
        cache_dirname = os.path.dirname(self._local_json)
        if not os.path.exists(cache_dirname):
            os.makedirs(cache_dirname)
        with open(self._local_json_path, "w", encoding="utf-8") as fle:
            fle.write(json.dumps(commits, indent=4, sort_keys=True))

    @property
    @abstractmethod
    def _local_json_path(self) -> str:
        """
        Returns:
            TYPE: The path to the JSON file that tracks the local commits.
        """
        return ""

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
