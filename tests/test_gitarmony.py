import os
import shutil
import tempfile
import unittest
import logging

from pprint import pprint
from git import Repo

from gitarmony.gitarmony import Gitarmony, Spread
from gitarmony.functions import is_read_only

from .functions import save_image


class GitarmonyTestCase(unittest.TestCase):
    """Sets up a temporary git repository for each test"""

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()
        logging.info("{} was created.".format(self.temp_dir))
        self.managed_remote = Repo.init(
            path=os.path.join(self.temp_dir, "managed.git"), bare=True
        )
        self.managed_clone = self.managed_remote.clone(
            os.path.join(self.temp_dir, "managed")
        )
        self.gitarmony_remote_url = os.path.join(self.temp_dir, "gitarmony.git")
        self.gitarmony_remote = Repo.init(path=self.gitarmony_remote_url, bare=True)
        self.gitarmony = Gitarmony.install(
            self.gitarmony_remote_url,
            self.managed_clone.working_dir,
            # Hooks are turned off because we would have to install Gitarmony CLI as
            # part of that test. Instead we are simulating the hooks operations below.
            update_hooks=False,
            update_gitignore=True,
        )

    def tearDown(self):
        if hasattr(self, "_outcome"):
            result = self.defaultTestResult()
            self._feedErrorsToResult(result, self._outcome.errors)
            error = self.list_to_reason(result.errors)
            failure = self.list_to_reason(result.failures)
            if not error and not failure:
                try:
                    shutil.rmtree(self.temp_dir)
                except PermissionError as error:
                    logging.error(error)

    def list_to_reason(self, exc_list):
        if exc_list and exc_list[-1][0] is self:
            return exc_list[-1][1]

    def test_config(self):
        config = self.gitarmony.config
        self.assertEqual(
            os.path.normpath(self.gitarmony_remote_url),
            os.path.normpath(config.get("remote_url")),
        )

    def test_local_only_commits(self):
        local_only_commits = self.gitarmony.local_only_commits
        working_dir = self.managed_clone.working_dir
        self.assertEqual(1, len(local_only_commits))
        self.assertEqual(2, len(local_only_commits[0]["changes"]))

        # Testing detecting un-tracked files.
        save_image(os.path.join(working_dir, "untracked_image_01.jpg"))

        # Testing detecting staged files.
        staged_image_01_path = os.path.join(working_dir, "staged_image_01.jpg")
        save_image(staged_image_01_path)
        self.managed_clone.index.add(staged_image_01_path)
        self.assertEqual(4, len(self.gitarmony.local_only_commits[0]["changes"]))

        commit = self.managed_clone.index.commit(message="Add staged_image.jpg")
        local_only_commits = self.gitarmony.local_only_commits
        self.assertEqual(2, len(local_only_commits))
        self.assertEqual(3, len(local_only_commits[0]["changes"]))
        self.assertEqual(1, len(local_only_commits[1]["changes"]))

        self.managed_clone.remote().push()
        local_only_commits = self.gitarmony.local_only_commits
        self.assertEqual(1, len(local_only_commits))
        self.assertEqual(3, len(local_only_commits[0]["changes"]))

        image_path = os.path.join(working_dir, "staged_image_02.jpg")
        save_image(image_path)
        # Simulating the application syncing when saving the file.
        self.gitarmony.update_tracked_commits()
        print("POST-SAVE TRACKED COMMITS")
        pprint(self.gitarmony.get_tracked_commits())

        self.managed_clone.index.add(image_path)
        self.managed_clone.index.commit(message="Add staged_image_02.jpg")
        # Simulating the post-commit hook.
        self.gitarmony.update_tracked_commits()
        print("POST-COMMIT TRACKED COMMITS")
        pprint(self.gitarmony.get_tracked_commits())

        self.managed_clone.remote().push()
        # Simulating a post-push hook.
        # It could only be implemented server-side as it's not an actual Git hook.
        self.gitarmony.update_tracked_commits()
        print("POST-PUSH TRACKED COMMITS")
        pprint(self.gitarmony.get_tracked_commits())

        # We just pushed the changes therefore there should be no missing commit.
        last_commit = self.gitarmony.get_file_last_commit("staged_image_02.jpg")
        spread = self.gitarmony.get_commit_spread(last_commit)
        self.assertEqual(
            Spread.LOCAL_ACTIVE_BRANCH | Spread.REMOTE_MATCHING_BRANCH, spread
        )

        # We are dropping the last commit locally.
        self.managed_clone.git.reset("--hard", commit.hexsha)
        # Simulating the post-checkout hook.
        self.gitarmony.update_tracked_commits()
        print("POST-CHECKOUT TRACKED COMMITS")
        pprint(self.gitarmony.get_tracked_commits())

        # As a result it should be a commit we do no have locally.
        last_commit = self.gitarmony.get_file_last_commit("staged_image_02.jpg")
        spread = self.gitarmony.get_commit_spread(last_commit)
        self.assertEqual(Spread.REMOTE_MATCHING_BRANCH, spread)

        self.assertEqual(False, is_read_only(staged_image_01_path))
        self.assertEqual(False, is_read_only(self.gitarmony.config_path))
        self.gitarmony.make_tracked_files_read_only(self.managed_clone.working_dir)
        self.assertEqual(True, is_read_only(staged_image_01_path))
        self.assertEqual(False, is_read_only(self.gitarmony.config_path))

        self.gitarmony.update_tracked_commits()

        missing_commit = self.gitarmony.make_file_writable(staged_image_01_path)
        self.assertIsInstance(missing_commit, type(None))
        missing_commit = self.gitarmony.make_file_writable(image_path)
        self.assertIsInstance(missing_commit, dict)
