import os
import shutil
import tempfile
import unittest
import logging

from gitarmony.functions import (
    is_binary_file,
    set_read_only,
    is_read_only,
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

    def test_set_read_only(self):
        image_path = os.path.join(self.temp_dir, "set_read_only.jpg")
        save_image(image_path)
        self.assertEqual(False, is_read_only(image_path))
        set_read_only(image_path, True)
        self.assertEqual(True, is_read_only(image_path))
        set_read_only(image_path, False)
        self.assertEqual(False, is_read_only(image_path))
        # This one is for coverage of non-existing files.
        set_read_only(os.path.join(self.temp_dir, "non_existing.jpg"), True)
