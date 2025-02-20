import os.path
import typing

import requests

from ..exceptions import StoreNotReachable, RepositoryInvalidConfig
from ..functions import modified_within, touch_file
from ..store import Store
from ..commit import Commit


class JsonbinStore(Store):
    """Implementation using JSONBin for storage."""

    def __init__(self, managed_repository):
        super().__init__(managed_repository)
        self._url: str = self._managed_repository.config.get("store_url", "")
        self._headers: dict = self._managed_repository.config.get("store_headers", {})
        if not isinstance(self._headers, dict):
            raise RepositoryInvalidConfig()
        self._timeout: float = 5

    @property
    def _local_json_path(self) -> str:
        return os.path.join(
            self._managed_repository.working_dir, ".gitalong", "commits.json"
        )

    @property
    def _pull_timestamp_path(self) -> str:
        return os.path.join(self._managed_repository.working_dir, ".gitalong", ".pull")

    @property
    def commits(self) -> typing.List[Commit]:
        headers = {}
        for key, value in self._headers.items():
            headers[key] = os.path.expandvars(value)
        pull_threshold = self._managed_repository.config.get("pull_threshold", 60)
        serializable_commits = []
        if modified_within(self._pull_timestamp_path, pull_threshold):
            return self._serializeables_to_commits(self._read_local_json())
        response = requests.get(self._url, headers=headers, timeout=self._timeout)
        if response.status_code != 200:
            raise StoreNotReachable(response.status_code, response.text)
        touch_file(self._pull_timestamp_path)
        serializable_commits = response.json()["record"]
        self._write_local_json(serializable_commits)
        return self._serializeables_to_commits(serializable_commits)

    @commits.setter
    def commits(self, commits: typing.List[Commit]):
        headers = {}
        for key, value in self._headers.items():
            headers[key] = os.path.expandvars(value)
        headers.update({"Content-Type": "application/json"})
        response = requests.put(
            self._url, headers=headers, json=commits, timeout=self._timeout
        )
        if response.status_code == 200:
            self._write_local_json(commits)
        else:
            raise StoreNotReachable()
