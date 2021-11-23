import os
import logging
import subprocess
import socket

from git import Repo
from git.exc import InvalidGitRepositoryError

from .change_set import ChangeSet
from .functions import find_repository_root
from .exceptions import GitarmonyNotInstalled


class Gitarmony:
    def __init__(self, managed_repository_path: str):
        """Summary

        Args:
            managed_repository_path (str): Description

        Raises:
            git.exc.GitarmonyNotInstalled: Description
        """
        managed_repository_path = find_repository_root(managed_repository_path)
        self._managed_repository = Repo(managed_repository_path)
        try:
            self._data_repository = Repo(
                os.path.join(managed_repository_path, ".gitarmony")
            )
        except InvalidGitRepositoryError:
            raise GitarmonyNotInstalled(
                "Gitarmony is not installed on this repository."
            )

    @classmethod
    def install(cls, managed_repository_path: str, data_repository_url: str):
        """Summary

        Args:
            managed_repository_path (str): Path to repository gitarmony should manage.
            data_repository_url (str): URL to the gitarmony data repository.

        Returns: Gitarmony: The gitarmony management class corresponding to the
            repository in which we just installed.
        """
        managed_repository_path = find_repository_root(managed_repository_path)
        managed_repository = Repo(managed_repository_path)
        managed_repository.create_submodule(".gitarmony", data_repository_url)
        gitarmony = cls(managed_repository_path)
        gitarmony.update_gitignore()
        gitarmony.install_hooks()
        gitarmony.install_actions()
        return

    @property
    def change_set(self) -> set:
        return ChangeSet(self._managed_repository, self._data_repository)

    def update_gitignore(self):
        """TODO: Adds gitarmony files to the repository's .gitignore."""
        return

    def install_hooks(self):
        """TODO: Install gitarmony hooks."""
        return

    def install_actions(self):
        """TODO: Install CI actions."""
        return

    # @property
    # def origin(self) -> str:
    #     return self._data_repository.remote(name="origin").url

    # @property
    # def user(self) -> str:
    #     """TODO: This is currently not returning the GitHub user which I think is an
    #     issue because people config could be all over the place."""
    #     if self._user is None:
    #         cmd = "git config --global user.name"
    #         logging.debug(cmd)
    #         output = subprocess.run(cmd, capture_output=True)
    #         logging.debug(output)
    #         self._user = output.stdout.decode("utf-8").replace("\n", "")
    #     return self._user or ""

    # @property
    # def email(self) -> str:
    #     if self._user is None:
    #         cmd = "git config --global user.email"
    #         logging.debug(cmd)
    #         output = subprocess.run(cmd, capture_output=True)
    #         self._email = output.stdout.decode("utf-8").replace("\n", "")
    #     return self._email or ""

    # @property
    # def host(self) -> str:
    #     """
    #     Returns:
    #         str: The host name.
    #     """
    #     return socket.gethostname()
