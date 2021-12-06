import os
import tempfile
import unittest
import logging

from git import Repo

from gitarmony.gitarmony import Gitarmony
from gitarmony.functions import is_read_only

from .functions import save_image


class GitarmonyTestCase(unittest.TestCase):
    """Sets up a temporary git repository for each test"""

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()
        logging.info("{} was created.".format(self.temp_dir))
        self.managed_origin = Repo.init(
            path=os.path.join(self.temp_dir, "managed.git"), bare=True
        )
        self.managed_clone = self.managed_origin.clone(
            os.path.join(self.temp_dir, "managed")
        )
        self.gitarmony_origin_url = os.path.join(self.temp_dir, "gitarmony.git")
        self.gitarmony_origin = Repo.init(path=self.gitarmony_origin_url, bare=True)
        self.gitarmony = Gitarmony.install(
            self.gitarmony_origin_url,
            self.managed_clone.working_dir,
            hooks=True,
        )

    def tearDown(self):
        try:
            # shutil.rmtree(self.temp_dir)
            pass
        except PermissionError as error:
            logging.error(error)

    def test_config(self):
        config = self.gitarmony.config
        self.assertEqual(
            os.path.normpath(self.gitarmony_origin_url),
            os.path.normpath(config["settings"]["origin"]),
        )

    def test_local_commits(self):
        local_commits = self.gitarmony.local_commits
        working_dir = self.managed_clone.working_dir
        self.assertEqual(1, len(local_commits))
        self.assertEqual(2, len(local_commits[0]["changes"]))

        # Testing detecting un-tracked files.
        save_image(os.path.join(working_dir, "untracked_image_01.jpg"))

        # Testing detecting staged files.
        staged_image_01_path = os.path.join(working_dir, "staged_image_01.jpg")
        save_image(staged_image_01_path)
        self.managed_clone.index.add(staged_image_01_path)
        self.assertEqual(4, len(self.gitarmony.local_commits[0]["changes"]))

        commit = self.managed_clone.index.commit(message="Add staged_image.jpg")
        local_commits = self.gitarmony.local_commits
        self.assertEqual(2, len(local_commits))
        self.assertEqual(3, len(local_commits[0]["changes"]))
        self.assertEqual(1, len(local_commits[1]["changes"]))

        self.managed_clone.remote().push()
        local_commits = self.gitarmony.local_commits
        self.assertEqual(1, len(local_commits))
        self.assertEqual(3, len(local_commits[0]["changes"]))

        image_path = os.path.join(working_dir, "staged_image_02.jpg")
        save_image(image_path)
        self.managed_clone.index.add(image_path)
        self.managed_clone.index.commit(message="Add staged_image_02.jpg")
        self.managed_clone.remote().push()
        conflicting_commit = self.gitarmony.get_conflicting_commit(
            "staged_image_02.jpg"
        )
        self.assertEqual(None, conflicting_commit)

        # We are dropping the last commit locally.
        self.managed_clone.git.reset("--hard", commit.hexsha)
        # As a result it should now be a conflicting commit for the given file.
        conflicting_commit = self.gitarmony.get_conflicting_commit(
            "staged_image_02.jpg"
        )
        self.assertIsInstance(conflicting_commit, dict)

        self.assertEqual(False, is_read_only(staged_image_01_path))
        self.assertEqual(False, is_read_only(self.gitarmony.config_path))
        self.gitarmony.make_binary_files_read_only()
        self.assertEqual(True, is_read_only(staged_image_01_path))
        self.assertEqual(False, is_read_only(self.gitarmony.config_path))

        self.gitarmony.update_tracked_commits()

        conflicting_commit = self.gitarmony.make_writable(staged_image_01_path)
        self.assertIsInstance(conflicting_commit, type(None))
        conflicting_commit = self.gitarmony.make_writable(image_path)
        self.assertIsInstance(conflicting_commit, dict)
