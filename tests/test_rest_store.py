import unittest

from unittest.mock import MagicMock, patch

import requests

from gitalong.stores.rest_store import RestStore


class RestStoreTestCase(unittest.TestCase):
    """Tests the REST API store."""

    @patch("requests.get")
    def test_get_request(self, mock_get):
        fake_response = MagicMock()
        fake_response.status_code = 200
        fake_response.json.return_value = {"key": "value"}
        mock_get.return_value = fake_response

        response = requests.get("https://fake-api.com/resource")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json(), {"key": "value"})

    @patch("requests.post")
    def test_post_request(self, mock_post):
        expected_data = {"key": "value"}
        fake_response = MagicMock()
        fake_response.status_code = 201
        mock_post.return_value = fake_response
        response = requests.post("https://fake-api.com/resource", json=expected_data)
        mock_post.assert_called_with(
            "https://fake-api.com/resource", json=expected_data
        )
        self.assertEqual(response.status_code, 201)

    # TODO: Implement the unit tests.
    # @patch("requests.get")
    # def test_get_commits(self, mock_get):
    #     fake_response = MagicMock()
    #     fake_response.status_code = 200
    #     fake_response.json.return_value = {"key": "value"}
    #     mock_get.return_value = fake_response

    #     store = RestStore()
    #     self.assertEqual(store.commits, {"key": "value"})

    # @patch("requests.post")
    # def test_set_commits(self, mock_post):
    #     fake_response = MagicMock()
    #     fake_response.status_code = 201
    #     mock_post.return_value = fake_response

    #     # TODO: Use the store.

    #     mock_post.assert_called_with(
    #         "https://fake-api.com/resource", json={"key": "value"}
    #     )
