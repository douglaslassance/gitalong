import os
import shutil
import logging

import configparser
import git
import dictdiffer

from .change_set import ChangeSet
from .functions import find_repository_root
from .exceptions import GitarmonyNotInstalled

from .functions import set_file_read_only


class Gitarmony:
    """The Gitarmony class aggregates all the Gitarmony actions that can happen on a
    repository.
    """

    def __init__(self, managed_repository: str = ""):
        """Summary

        Args:
            managed_repository (str, optional):
                The managed repository. Current working directory if not passed.

        Raises:
            gitarmony.exceptions.GitarmonyNotInstalled: Description

        No Longer Raises:
            git.exc.GitarmonyNotInstalled: Description
        """
        managed_repository = find_repository_root(managed_repository)
        self._managed_repository = git.Repo(managed_repository)
        self._change_set = None
        try:
            self._gitarmony_repository = git.Repo(
                os.path.join(managed_repository, ".gitarmony")
            )
        except git.exc.InvalidGitRepositoryError as error:
            raise GitarmonyNotInstalled(
                "Gitarmony is not installed on this repository."
            ) from error

    @classmethod
    def install(
        cls,
        gitarmony_repository: str,
        managed_repository: str = "",
        platform: str = "",
        secret: str = "",
    ):
        """
        Args:
            gitarmony_repository (str):
                The URL of the repository that will store gitarmony data.
            managed_repository (str, optional):
                The repository in which we install Gitarmony. Defaults to current
                working directory. Current working directory if not passed.
            platform (str, optional):
                The platform where origin is stored. This will setup CI actions. Only
                supports `github`.
            secret (str, optional):
                The name of the secret (not the value) that will hold the personal
                access token that grants access to your Gitarmony repository.

        Deleted Parameters:
            Returns: Gitarmony:
                The gitarmony management class corresponding to the repository in which
                we just installed.
        """
        managed_repository = find_repository_root(managed_repository)
        managed_repository = git.Repo(managed_repository)
        managed_repository.create_submodule(".gitarmony", gitarmony_repository)
        gitarmony = cls(managed_repository.working_dir)
        gitarmony.update_gitignore()
        gitarmony.install_hooks()
        gitarmony.install_actions()

    @property
    def change_set(self) -> ChangeSet:
        if self._change_set == None:
            self._change_set = ChangeSet(
                self._managed_repository, self._gitarmony_repository
            )
        return self._change_set

    def update_gitignore(self):
        """Update the .gitignore of the managed repository with Gitarmony directives.

        TODO: Improve update by considering what is already ignored.
        """
        with open(
            os.path.join(self._managed_repository.working_dir, ".gitignore"), "w"
        ) as gitignore:
            gitignore_content = gitignore.read()
            with open(
                os.path.join(os.path.dirname(__file__), "resources", "gitignore")
            ) as patch:
                patch_content = patch.read()
            if patch_content not in gitignore_content:
                gitignore.write(gitignore_content + patch_content)

    @property
    def config(self) -> dict:
        """
        Returns:
            dict: The content of `.gitarmony.cfg` as a dictionary.
        """
        config = configparser.ConfigParser()
        config.read(os.path.join(self._gitarmony_repository.working_dir, ".gitarmony"))
        return {}

    def sync(self):
        """Synchronize the change set with origin."""
        self.change_set.sync()

    @property
    def hooks_path(self):
        try:
            basename = self._managed_repository.config_reader().get_value(
                "core", "hooksPath"
            )
        except configparser.NoOptionError:
            basename = os.path.join(".git", "hooks")
        return os.path.normpath(
            os.path.join(self._managed_repository.working_dir, basename)
        )

    def install_hooks(self):
        """Installs Gitarmony hooks in managed repository."""
        source = os.path.join(os.path.dirname(__file__), "resources", "hooks")
        destination = self.hooks_path
        for (_, __, filenames) in os.walk(source):
            for filename in filenames:
                shutil.copyfile(filename, destination)

    def make_writable(self, filename: str) -> dict:
        """
        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the current working directory.

        Returns:
            dict: The conflicting changes if making the file writable was not possible.
        """
        filename = os.path.abspath(filename)
        if not os.path.isfile(filename):
            logging.error("File does not exists.")
            return
        conflicting_changes = self.change_set.conflicting_changes(filename)
        if not conflicting_changes:
            set_file_read_only(filename, False)
            logging.info("Made {} writable with success!")
        return conflicting_changes

    def install_actions(self, platform: str = ""):
        """Install CI actions.

        TODO: Add GitLab support.

        Args: platform (str, optional):
            The platform to install actions for. Will not install any actions if not
            passed.
        """
        source = os.path.join(
            os.path.dirname(__file__), "resources", "actions", platform
        )
        if not os.path.isdir(source):
            logging.error("Could not install actions for platform {}".format(platform))
        if platform.lower() == "github":
            destination = os.path.join(
                self._managed_repository.working_dir, ".github", "workflows"
            )
            if not os.path.isdir(destination):
                os.makedirs(destination)
            for (_, __, filenames) in os.walk(source):
                for filename in filenames:
                    shutil.copyfile(filename, destination)
