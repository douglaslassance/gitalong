import unittest
import logging
import shutil
import os
import tempfile

from git.repo import Repo


class GitalongCase(unittest.TestCase):

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()
        logging.info(self.temp_dir)
        self.managed_remote = Repo.init(
            path=os.path.join(self.temp_dir, "managed.git"), bare=True
        )
        self.managed_clone = self.managed_remote.clone(
            os.path.join(self.temp_dir, "managed")
        )
        self.store_url = os.path.join(self.temp_dir, "store.git")
        self.store_remote = Repo.init(path=self.store_url, bare=True)

    def tearDown(self):
        try:
            shutil.rmtree(self.temp_dir)
        except PermissionError as error:
            logging.error(error)
