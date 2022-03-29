import os
import shutil
import unittest
import logging
import tempfile
import socket
import getpass

from click.testing import CliRunner
from git.repo import Repo

from gitalong import cli  # noqa: E402 pylint: disable=wrong-import-position

from .functions import save_image  # noqa: E402


class CliTestCase(unittest.TestCase):
    """Sets up a temporary git repository for each test"""

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp()
        logging.info(self.temp_dir)
        self.managed_remote = Repo.init(
            path=os.path.join(self.temp_dir, "managed.git"), bare=True
        )
        self.managed_clone = self.managed_remote.clone(
            os.path.join(self.temp_dir, "managed")
        )
        self.store_remote = Repo.init(
            path=os.path.join(self.temp_dir, "store.git"), bare=True
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

    def test_commands(self):
        working_dir = self.managed_clone.working_dir
        obj = {"REPOSITORY": working_dir}

        runner = CliRunner()
        result = runner.invoke(
            cli.setup,
            [
                self.store_remote.working_dir,
                "--track-uncommitted",
                "--track-binaries",
                "--modify-permissions",
                "--update-gitignore",
            ],
            obj=obj,
        )
        self.assertEqual(0, result.exit_code, result.output)

        config_path = os.path.join(working_dir, ".gitalong.json")
        self.assertEqual(True, os.path.exists(config_path))

        # Testing detecting un-tracked files.
        untracked_image_01 = os.path.join(working_dir, "untracked_image_01.jpg")
        save_image(untracked_image_01)

        result = runner.invoke(cli.update, obj=obj)
        self.assertEqual(0, result.exit_code, result.output)

        result = runner.invoke(cli.status, [untracked_image_01], obj=obj)
        self.assertEqual(0, result.exit_code, result.output)
        host = socket.gethostname()
        user = getpass.getuser()
        output = f"+------- {untracked_image_01} - - - {host} {user}\n"
        self.assertEqual(output, result.output)
