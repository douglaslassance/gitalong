# pylint: disable=missing-function-docstring
# pylint: disable=missing-class-docstring

import os
import shutil
import tempfile
import unittest
import logging

from gitalong.functions import (
    is_binary_file,
    get_filenames_from_move_string,
)

from .functions import save_image


class FunctionsTestCase(unittest.TestCase):
    """Sets up a temporary git repository for each test"""

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()

    def tearDown(self):
        try:
            shutil.rmtree(self.temp_dir)
        except PermissionError as error:
            logging.error(error)

    def test_is_binary_file(self):
        self.assertEqual(False, is_binary_file(__file__))
        image_path = os.path.join(self.temp_dir, "image.jpg")
        save_image(image_path)
        self.assertEqual(True, is_binary_file(image_path))

    def test_get_filenames_from_move_string(self):
        move_string = get_filenames_from_move_string("A/B/C.abc")
        self.assertEqual(("A/B/C.abc",), move_string)

        move_string = get_filenames_from_move_string("A/B/{C.abc => D.abc}")
        self.assertEqual(("A/B/C.abc", "A/B/D.abc"), move_string)

        move_string = get_filenames_from_move_string("A/B/{C..abc => C.abc}")
        self.assertEqual(("A/B/C..abc", "A/B/C.abc"), move_string)

        move_string = get_filenames_from_move_string("A/B/{C/D.abc => E/F.abc}")
        self.assertEqual(("A/B/C/D.abc", "A/B/E/F.abc"), move_string)
