import typing
import json
import os
import requests

from ..store import Store


class RestStore(Store):
    """Implementation using a Rest API access point as storage."""

    _timeout = 5

    def __init__(self, managed_repository):
        super().__init__(managed_repository)
        config = self._managed_repository.config
        self._url = config.get("stored_url", "")
        # Feeling the stringified headers with environment variables.
        self._headers = json.loads(
            os.path.expandvars(json.dumps(config.get("store_headers", {})))
        )

    @property
    def commits(self) -> typing.List[dict]:
        response = requests.get(self._url, headers=self._headers, timeout=self._timeout)
        return response.json()

    @commits.setter
    def commits(self, commits: typing.List[dict]):
        requests.post(
            self._url, json=commits, headers=self._headers, timeout=self._timeout
        )
