# pylint: disable=missing-function-docstring
# pylint: disable=missing-class-docstring
# pylint: disable=attribute-defined-outside-init

import getpass
import logging
import os
import shutil
import socket
import unittest
import asyncio

from click.testing import CliRunner
from git.repo import Repo

from gitalong import Repository, CommitSpread, RepositoryNotSetup, cli, batch
from gitalong.functions import is_writeable

from .functions import save_image


class GitalongCase(unittest.TestCase):

    __test__ = False

    def _setup_repository(self, temp_dir, store_url, store_headers=None):
        self.temp_dir = temp_dir
        logging.info(self.temp_dir)
        self._managed_remote = Repo.init(
            path=os.path.join(self.temp_dir, "managed.git"), bare=True
        )
        self._managed_clone = self._managed_remote.clone(
            os.path.join(self.temp_dir, "managed")
        )
        self._store_url = store_url
        self._store_headers = store_headers or {}

        self.assertRaises(
            RepositoryNotSetup, Repository, self._managed_clone.working_dir
        )

        self.repository = Repository.setup(
            store_url=store_url,
            store_headers=store_headers,
            managed_repository=str(self._managed_clone.working_dir),
            modify_permissions=True,
            track_binaries=True,
            track_uncommitted=True,
            update_gitignore=True,
            # Hooks are turned off because we would have to install Gitalong CLI as
            # part of that test. Instead, we are simulating the hooks operations below.
            update_hooks=False,
        )

    def test_config(self):
        config = self.repository.config
        self.assertEqual(
            os.path.normpath(self._store_url),
            os.path.normpath(config.get("store_url", "")),
        )

    def test_lib(self):  # pylint: disable=too-many-statements
        local_only_commits = asyncio.run(batch.get_local_only_commits(self.repository))
        self.assertEqual(1, len(local_only_commits))
        self.assertEqual(2, len(local_only_commits[0]["changes"]))

        # Make an initial commit.
        self._managed_clone.index.add(".gitignore")
        self._managed_clone.index.add(".gitalong.json")
        self._managed_clone.index.commit(message="Initial commit")

        # Simulating a post commit update.
        asyncio.run(batch.update_tracked_commits(self.repository))

        # Checking commits.
        local_only_commits = asyncio.run(batch.get_local_only_commits(self.repository))
        self.assertEqual(1, len(local_only_commits))

        # Make a new binary file.
        working_dir = self._managed_clone.working_dir
        image_name = "image.jpg"
        image_path = os.path.join(working_dir, image_name)
        save_image(image_path)

        # Simulating a post save update.
        asyncio.run(batch.update_tracked_commits(self.repository))
        local_only_commits = asyncio.run(batch.get_local_only_commits(self.repository))
        self.assertEqual(2, len(local_only_commits))
        # The fist local only commit is always the one holding uncommitted changes.
        self.assertEqual(
            1,
            len(
                asyncio.run(batch.get_local_only_commits(self.repository))[0]["changes"]
            ),
        )

        # Staging the file.
        self._managed_clone.index.add(image_path)

        # Simulating a post stage update.
        asyncio.run(batch.update_tracked_commits(self.repository))

        # Checking commits.
        self.assertEqual(2, len(local_only_commits))
        # The fist local only commit is always the one holding uncommitted changes.
        self.assertEqual(
            1,
            len(
                asyncio.run(batch.get_local_only_commits(self.repository))[0]["changes"]
            ),
        )
        last_commits = asyncio.run(batch.get_files_last_commits([image_path]))
        self.assertEqual(1, len(last_commits))
        last_commit = last_commits[0]
        self.assertEqual(CommitSpread.MINE_UNCOMMITTED, last_commit.commit_spread)

        # Checking permissions.
        self.assertEqual(True, is_writeable(self.repository.config_path))
        self.assertEqual(True, is_writeable(image_path))

        # Committing the file.
        add_image_commit = self._managed_clone.index.commit(message=f"Add {image_name}")

        # Simulating the post-commit hook.
        asyncio.run(batch.update_tracked_commits(self.repository))

        # Checking commits.
        local_only_commits = asyncio.run(batch.get_local_only_commits(self.repository))
        self.assertEqual(2, len(local_only_commits))
        self.assertEqual(1, len(local_only_commits[0]["changes"]))
        self.assertEqual(2, len(local_only_commits[1]["changes"]))
        last_commits = asyncio.run(batch.get_files_last_commits([image_path]))
        self.assertEqual(1, len(last_commits))
        self.assertEqual(CommitSpread.MINE_ACTIVE_BRANCH, last_commits[0].commit_spread)

        # Checking permissions.
        self.assertEqual(True, is_writeable(self.repository.config_path))
        self.assertEqual(True, is_writeable(image_path))

        # Pushing the change.
        self._managed_clone.remote().push()

        # Simulating a post-push hook.
        # It could only be implemented server-side as it's not an actual Git hook.
        asyncio.run(batch.update_tracked_commits(self.repository))

        # Checking commits.
        local_only_commits = asyncio.run(batch.get_local_only_commits(self.repository))
        self.assertEqual(0, len(local_only_commits))
        last_commits = asyncio.run(batch.get_files_last_commits([image_path]))
        self.assertEqual(1, len(last_commits))
        self.assertEqual(
            CommitSpread.MINE_ACTIVE_BRANCH | CommitSpread.REMOTE_MATCHING_BRANCH,
            last_commits[0].commit_spread,
        )

        # Checking permissions.
        self.assertEqual(True, is_writeable(self.repository.config_path))
        self.assertEqual(False, is_writeable(image_path))

        # Claim the file for changes.
        blocking_commits = asyncio.run(batch.claim_files([image_path]))
        self.assertEqual(1, len(blocking_commits))
        self.assertEqual(False, bool(blocking_commits[0]))

        # Modifying and committing and pushing the change.
        save_image(image_path, color=(255, 255, 255))
        self._managed_clone.index.add(image_path)
        self._managed_clone.index.commit(message=f"Modify {image_name}")
        self._managed_clone.remote().push()

        # Dropping the last commit.
        self._managed_clone.git.reset("--hard", add_image_commit.hexsha)

        # Simulating the post-checkout hook.
        asyncio.run(batch.update_tracked_commits(self.repository))

        # Checking commits.
        last_commits = asyncio.run(batch.get_files_last_commits([image_path]))
        self.assertEqual(1, len(last_commits))
        self.assertEqual(
            CommitSpread.REMOTE_MATCHING_BRANCH, last_commits[0].commit_spread
        )

        # Checking permissions.
        self.assertEqual(True, is_writeable(self.repository.config_path))
        self.assertEqual(False, is_writeable(image_path))

    def test_cli(self):
        working_dir = self._managed_clone.working_dir
        obj = {"REPOSITORY": working_dir}

        runner = CliRunner()

        args = [self._store_url]
        for key, value in self._store_headers.items():
            args += ["--store-header", f"{key}={value}"]
        args += [
            "--track-uncommitted",
            "--track-binaries",
            "--modify-permissions",
            "--update-gitignore",
        ]

        result = runner.invoke(
            cli.setup,
            args,
            obj=obj,
        )
        self.assertEqual(0, result.exit_code, result.output)

        # Testing configuration.
        config_path = os.path.join(working_dir, ".gitalong.json")
        self.assertEqual(True, os.path.exists(config_path))

        # Creating binary file.
        image_path = os.path.join(working_dir, "image.jpg")
        save_image(image_path)

        # Testing update.
        result = runner.invoke(cli.update, obj=obj)
        self.assertEqual(0, result.exit_code, result.output)

        # Testing status.
        result = runner.invoke(cli.status, [image_path], obj=obj)
        self.assertEqual(0, result.exit_code, result.output)
        host = socket.gethostname()
        user = getpass.getuser()
        output = f"+------- {image_path} - - - {host} {user}\n"
        self.assertEqual(output, result.output)

    def tearDown(self):
        try:
            shutil.rmtree(self.temp_dir)
        except PermissionError as error:
            logging.error(error)
