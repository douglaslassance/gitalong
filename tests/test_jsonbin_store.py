import tempfile
from unittest.mock import patch, MagicMock

from .cases import GitalongCase


class JsonbinStoreTestCase(GitalongCase):

    __test__ = True

    def setUp(self):
        temp_dir = tempfile.mkdtemp()
        url = f"https://api.jsonbin.io/v3/b/1234/"
        headers = {"X-Master-Key": "5678"}

        self._stored_value = {"record": {}}

        self.get_patcher = patch("requests.get", self.get_patch)
        self.get_mock = self.get_patcher.start()

        self.put_patcher = patch("requests.put", self.put_patch)
        self.put_mock = self.put_patcher.start()

        self.setup_repository(temp_dir, url, headers)

    def get_patch(self, url, headers=None):
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = self._stored_value
        return mock_response

    def put_patch(self, url, headers=None, json=None):
        self._stored_value = {"record": json or {}}
        mock_response = MagicMock()
        mock_response.status_code = 200
        return mock_response

    def tearDown(self):
        super(JsonbinStoreTestCase, self).tearDown()
        self.get_patcher.stop()
        self.put_patcher.stop()
