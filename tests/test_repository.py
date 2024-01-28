import os
import shutil
import tempfile
import unittest
import logging

from git.repo import Repo

from gitalong import Repository, CommitSpread, RepositoryNotSetup
from gitalong.functions import is_read_only

from .gitalong_case import GitalongCase
from .functions import save_image


class RepositoryTestCase(GitalongCase):
    """Sets up a temporary git repository for each test"""

    def setUp(self):
        super(RepositoryTestCase, self).setUp()

        self.assertRaises(
            RepositoryNotSetup, Repository, self.managed_clone.working_dir
        )

        self.repository = Repository.setup(
            self.store_remote.working_dir,
            store_headers={},
            managed_repository=self.managed_clone.working_dir,
            modify_permissions=True,
            track_binaries=True,
            track_uncommitted=True,
            update_gitignore=True,
            # Hooks are turned off because we would have to install Gitalong CLI as
            # part of that test. Instead, we are simulating the hooks operations below.
            update_hooks=False,
        )

    def list_to_reason(self, exc_list):
        if exc_list and exc_list[-1][0] is self:
            return exc_list[-1][1]

    def test_config(self):
        config = self.repository.config
        self.assertEqual(
            os.path.normpath(self.store_url),
            os.path.normpath(config.get("store_url")),
        )

    def test_workflow(self):
        local_only_commits = self.repository.local_only_commits
        working_dir = self.managed_clone.working_dir
        self.assertEqual(1, len(local_only_commits))
        self.assertEqual(2, len(local_only_commits[0]["changes"]))

        # Testing detecting un-tracked files.
        save_image(os.path.join(working_dir, "untracked_image_01.jpg"))

        # Testing detecting staged files.
        staged_image_01_path = os.path.join(working_dir, "staged_image_01.jpg")
        save_image(staged_image_01_path)
        self.managed_clone.index.add(staged_image_01_path)
        self.assertEqual(4, len(self.repository.local_only_commits[0]["changes"]))

        commit = self.managed_clone.index.commit(message="Add staged_image.jpg")
        local_only_commits = self.repository.local_only_commits
        self.assertEqual(2, len(local_only_commits))
        self.assertEqual(3, len(local_only_commits[0]["changes"]))
        self.assertEqual(1, len(local_only_commits[1]["changes"]))

        self.managed_clone.remote().push()
        local_only_commits = self.repository.local_only_commits
        self.assertEqual(1, len(local_only_commits))
        self.assertEqual(3, len(local_only_commits[0]["changes"]))

        image_path = os.path.join(working_dir, "staged_image_02.jpg")
        save_image(image_path)
        # Simulating the application syncing when saving the file.
        self.repository.update_tracked_commits()
        # print("POST-SAVE TRACKED COMMITS")
        # pprint(self.repository.get_tracked_commits())

        self.managed_clone.index.add(image_path)
        self.managed_clone.index.commit(message="Add staged_image_02.jpg")
        # Simulating the post-commit hook.
        self.repository.update_tracked_commits()
        # print("POST-COMMIT TRACKED COMMITS")
        # pprint(self.repository.get_tracked_commits())

        self.managed_clone.remote().push()
        # Simulating a post-push hook.
        # It could only be implemented server-side as it's not an actual Git hook.
        self.repository.update_tracked_commits()
        # print("POST-PUSH TRACKED COMMITS")
        # pprint(self.repository.get_tracked_commits())

        # We just pushed the changes therefore there should be no missing commit.
        last_commit = self.repository.get_file_last_commit("staged_image_02.jpg")
        spread = self.repository.get_commit_spread(last_commit)
        self.assertEqual(
            CommitSpread.MINE_ACTIVE_BRANCH | CommitSpread.REMOTE_MATCHING_BRANCH,
            spread,
        )

        # We are dropping the last commit locally.
        self.managed_clone.git.reset("--hard", commit.hexsha)
        # Simulating the post-checkout hook.
        self.repository.update_tracked_commits()
        # print("POST-CHECKOUT TRACKED COMMITS")
        # pprint(self.repository.get_tracked_commits())

        # As a result it should be a commit we do no have locally.
        last_commit = self.repository.get_file_last_commit("staged_image_02.jpg")
        spread = self.repository.get_commit_spread(last_commit)
        self.assertEqual(CommitSpread.REMOTE_MATCHING_BRANCH, spread)

        self.assertEqual(False, is_read_only(staged_image_01_path))
        self.assertEqual(False, is_read_only(self.repository.config_path))
        self.repository.update_file_permissions(staged_image_01_path)
        self.assertEqual(True, is_read_only(staged_image_01_path))
        self.repository.update_file_permissions(self.repository.config_path)
        self.assertEqual(False, is_read_only(self.repository.config_path))

        self.repository.update_tracked_commits()

        missing_commit = self.repository.make_file_writable(staged_image_01_path)
        self.assertEqual(False, bool(missing_commit))
        missing_commit = self.repository.make_file_writable(image_path)
        self.assertEqual(True, bool(missing_commit))
