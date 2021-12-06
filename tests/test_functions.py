import os
import shutil
import tempfile
import unittest

from gitarmony.functions import is_binary_file, is_ci_runner_host

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
        image_path = os.path.join(self.temp_dir, 'image.jpg')
        save_image(image_path)
        self.assertEqual(True, is_binary_file(image_path))

    def test_is_runner_host(self):
        os.environ['CI'] = "1"
        self.assertEqual(True, is_ci_runner_host())
        del os.environ['CI']
        self.assertEqual(False, is_ci_runner_host())
