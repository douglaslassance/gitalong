import os.path
import typing

import requests

from ..exceptions import StoreNotReachable
from ..store import Store


class JsonbinStore(Store):
    """Implementation using JSONBin for storage."""

    def __init__(self, managed_repository):
        super().__init__(managed_repository)
        self._url: str = self._managed_repository.config.get("STORE_URL", "")
        self._headers: dict = self._managed_repository.config.get("STORE_HEADERS", {})

    @property
    def commits(self) -> typing.List[dict]:
        response = requests.get(self._url, headers=self._headers)
        if response.status_code == 200:
            return response.json()["record"]
        else:
            raise StoreNotReachable()

    @commits.setter
    def commits(self, commits: typing.List[dict]):
        headers = {}
        for key, value in self._headers.items():
            headers[key] = os.path.expandvars(value)
        headers.update({"Content-Type": "application/json"})
        response = requests.put(self._url, headers=self._headers, json=commits)
        if response.status_code != 200:
            raise StoreNotReachable()
