import configparser
import datetime
import getpass
import json
import logging
import os
import shutil
import socket

from typing import Optional, List

import git
import git.exc

from git.repo import Repo

from .store import Store
from .stores.git_store import GitStore
from .stores.jsonbin_store import JsonbinStore
from .exceptions import RepositoryNotSetup, RepositoryInvalidConfig
from .functions import (
    get_real_path,
    is_binary_file,
    pulled_within,
)


class Repository:  # pylint: disable=too-many-public-methods
    """Aggregates all the Gitalong actions that can happen on a Git repository.

    Raises:
        git.exc.InvalidGitRepositoryError: If the path is not in or the Git clone.
    """

    _instances = {}
    _config_basename = ".gitalong.json"

    def __new__(
        cls,
        repository: str = "",
        use_cached_instances=False,
    ):
        managed_repo = Repo(repository, search_parent_directories=True)
        working_dir = managed_repo.working_dir
        if use_cached_instances:
            return cls._instances.setdefault(working_dir, super().__new__(cls))
        return super().__new__(cls)

    def __init__(
        self,
        repository: str = "",
        use_cached_instances=False,  # pylint: disable=unused-argument
    ):
        """
        Args:
            repository (str):
                The managed repository exact absolute path.
            use_cached_instances (bool):
                If true, the class will return "singleton" cached per clone.

        Raises:
            RepositoryInvalidConfig: If the store URL is not valid.
            RepositoryNotSetup: If Gitalong is not installed on the repository.
        """
        self._config = None
        self._submodules = None

        self._managed_repository = Repo(repository, search_parent_directories=True)
        self._remote = self._managed_repository.remote()

        store_url = self.config.get("store_url", "")
        if store_url.startswith("https://api.jsonbin.io"):
            self._store = JsonbinStore(self)
        elif store_url.endswith(".git"):
            self._store = GitStore(self)
        else:
            raise RepositoryInvalidConfig("Invalid store URL in configuration.")

        if self.config.get(
            "modify_permissions", False
        ) and self._managed_repository.config_reader().get_value(
            "core", "fileMode", True
        ):
            config_writer = self._managed_repository.config_writer()
            config_writer.set_value("core", "fileMode", "false")
            config_writer.release()

    @classmethod
    def setup(  # pylint: disable=too-many-positional-arguments
        cls,
        store_url: str,
        store_headers: Optional[dict] = None,
        managed_repository: str = "",
        modify_permissions=False,
        pull_threshold: float = 60.0,
        track_binaries: bool = False,
        track_uncommitted: bool = False,
        tracked_extensions: Optional[List[str]] = None,
        update_gitignore: bool = False,
        update_hooks: bool = False,
    ):
        """Setup Gitalong in a repository.

        Args:
            store_url (str):
                The URL or path to the repository that Gitalong will use to store local
                changes.
            store_headers (str):
                The headers to connect to the API end point.
            managed_repository (str, optional):
                The repository in which we install Gitalong. Defaults to current
                working directory. Current working directory if not passed.
            modify_permissions (bool, optional):
                Whether Gitalong should manage permissions of binary files.
            track_binaries (bool, optional):
                Track all binary files by automatically detecting them.
            track_uncommitted (bool, optional):
                Track uncommitted changes. Better for collaboration but requires to push
                tracked commits after each file system operation.
            tracked_extensions (List[str], optional):
                List of extensions to track.
            pull_threshold (list, optional):
                Time in seconds that need to pass before Gitalong pulls again. Defaults
                to 10 seconds. This is for optimization's sake as pull and fetch
                operation are expensive. Defaults to 60 seconds.
            update_gitignore (bool, optional):
                Whether .gitignore should be modified in the managed repository to
                ignore Gitalong files.
            update_hooks (bool, optional):
                Whether hooks should be updated with Gitalong logic.

        Returns:
            Gitalong:
                The Gitalong management class corresponding to the repository in
                which we just installed.
        """
        tracked_extensions = tracked_extensions or []
        managed_repo = Repo(managed_repository, search_parent_directories=True)
        config_path = os.path.join(managed_repo.working_dir, cls._config_basename)
        config = {
            "store_url": store_url,
            "store_headers": store_headers or {},
            "modify_permissions": modify_permissions,
            "track_binaries": track_binaries,
            "tracked_extensions": tracked_extensions,
            "pull_threshold": pull_threshold,
            "track_uncommitted": track_uncommitted,
        }
        cls._write_config_file(config, config_path)
        gitalong = cls(repository=str(managed_repo.working_dir))
        if modify_permissions:
            managed_repo.config_writer().set_value("core", "fileMode", "true").release()
        if update_gitignore:
            gitalong.update_gitignore()
        if update_hooks:
            gitalong.install_hooks()
        return gitalong

    @classmethod
    def from_filename(cls, filename: str) -> Optional["Repository"]:
        """
        Args:
            filename (str):
                Existing absolute path to a file or folder in the managed repository.
                That includes the managed repository itself.

        Returns:
            Optional[Repository]: The repository or None.
        """
        try:
            return cls(repository=filename, use_cached_instances=True)
        except (
            git.exc.InvalidGitRepositoryError,
            RepositoryNotSetup,
            git.exc.NoSuchPathError,
        ):
            return None

    @staticmethod
    def _write_config_file(config: dict, path: str):
        with open(path, "w", encoding="utf8") as config_file:
            json.dump(config, config_file, indent=4, sort_keys=True)

    def update_gitignore(self):
        """Update the .gitignore of the managed repository with Gitalong directives."""
        # TODO: Improve by considering what is already ignored.
        gitignore_path = os.path.join(self.working_dir, ".gitignore")
        content = ""
        if os.path.exists(gitignore_path):
            with open(gitignore_path, encoding="utf8") as gitignore:
                content = gitignore.read()
        with open(gitignore_path, "w", encoding="utf8") as gitignore:
            with open(
                os.path.join(os.path.dirname(__file__), "resources", "gitignore"),
                encoding="utf8",
            ) as patch:
                patch_content = patch.read()
            if patch_content not in content:
                gitignore.write(content + patch_content)

    @property
    def config_path(self) -> str:
        """
        Returns:
            str: The path to the configuration file.
        """
        return os.path.join(self.working_dir, self._config_basename)

    @property
    def store(self) -> Store:
        """
        Returns:
            Store: The store that Gitalong uses to keep track of local changes.
        """
        return self._store

    @property
    def config(self) -> dict:
        """
        Returns:
            dict: The content of `.gitalong.json` as a dictionary.
        """
        if self._config is None:
            try:
                with open(self.config_path, encoding="utf8") as _config_file:
                    self._config = json.loads(_config_file.read())
            except FileNotFoundError as error:
                raise RepositoryNotSetup(
                    "Gitalong is not installed on this repository."
                ) from error
        return self._config

    @property
    def remote(self) -> git.Remote:
        """
        Returns:
            git.Remote: The remote repository of the managed repository.
        """
        return self._remote

    @property
    def remote_url(self) -> str:
        """
        Returns:
            str: The URL of the remote repository.
        """
        return self._remote.url

    @property
    def hooks_path(self) -> str:
        """
        Returns:
            str: The hook path of the managed repository.
        """
        try:
            basename = self._managed_repository.config_reader().get_value(
                "core", "hooksPath"
            )
        except configparser.NoOptionError:
            basename = os.path.join(".git", "hooks")
        path = os.path.join(
            self.working_dir,
            basename,  # pyright: ignore[reportCallIssue, reportArgumentType]
        )
        return os.path.normpath(path)

    def install_hooks(self):
        """Installs Gitalong hooks in managed repository.

        TODO: Implement non-destructive version of these hooks. Currently we don't have
        any consideration for pre-existing content.
        """
        hooks = os.path.join(os.path.dirname(__file__), "resources", "hooks")
        destination_dir = self.hooks_path
        for dirname, _, basenames in os.walk(hooks):
            for basename in basenames:
                filename = os.path.join(dirname, basename)
                destination = os.path.join(destination_dir, basename)
                msg = f"Copying hook from {filename} to {destination}"
                logging.info(msg)
                shutil.copyfile(filename, destination)

    def get_relative_path(self, filename: str) -> str:
        """
        Args:
            filename (str): The absolute path.

        Returns:
            str: The path relative to the managed repository.
        """
        if os.path.exists(filename):
            filename = os.path.relpath(filename, self.working_dir)
        return filename

    def get_absolute_path(self, filename: str) -> str:
        """
        Args:
            filename (str): The path relative to the managed repository.

        Returns:
            str: The absolute file system path.
        """
        if os.path.exists(filename):
            return filename
        return os.path.join(self.working_dir, filename)

    def get_uncommitted_changes_commit(
        self, claims: Optional[List[str]] = None
    ) -> dict:
        """
        Returns:
            dict: Returns a commit dictionary representing uncommitted changes.
        """
        uncommitted_changes = self._uncommitted_changes + (claims or [])
        if not uncommitted_changes:
            return {}
        commit = {
            "remote": self._remote.url,
            "changes": self._uncommitted_changes,
            "date": str(datetime.datetime.now()),
        }
        commit.update(self.context_dict)
        return commit

    @property
    def context_dict(self) -> dict:
        """
        Returns:
            dict: A dict of contextual values that we attached to tracked commits.
        """
        return {
            "host": socket.gethostname(),
            "user": getpass.getuser(),
            "clone": get_real_path(self.working_dir),
        }

    def is_remote_commit(self, hexsha: str) -> bool:
        """Whether a commit is on a remote branch."""
        return self._managed_repository.git.branch("--remotes", "--contains", hexsha)

    @property
    def branches(self) -> List[git.Head]:
        """
        Returns:
            list: A list of branches in the managed repository.
        """
        return self._managed_repository.branches

    @property
    def _uncommitted_changes(self) -> List[str]:
        """
        Returns:
            list: A list of unique relative filenames that feature uncommitted changes.
        """
        git_cmd = self._managed_repository.git
        output = git_cmd.ls_files("--exclude-standard", "--others")
        untracked_changes = output.split("\n") if output else []
        output = git_cmd.diff("--name-only")
        changes = output.split("\n") if output else []
        output = git_cmd.diff("--staged", "--name-only")
        staged_changes = output.split("\n") if output else []
        # A file can be in both in untracked and staged changes.
        return list(set(untracked_changes + changes + staged_changes))

    def _is_ignored(self, filename: str) -> bool:
        """
        Args:
            filename (str): The filename to check for.

        Returns:
            bool: Whether a file is ignored by the managed repository .gitignore file.
        """
        filename = self.get_relative_path(filename)
        try:
            self._managed_repository.git.check_ignore(filename)
            return True
        except git.exc.GitCommandError:
            return False

    @property
    def files(self) -> list:
        """
        Returns:
            list:
                The relative filenames that are tracked by the managed repository. Not
                to be confused with the files tracked by Gitalong.
        """
        git_cmd = self._managed_repository.git
        try:
            # TODO: HEAD might not be safe. The user could checkout an earlier commit.
            filenames = git_cmd.ls_tree(full_tree=True, name_only=True, r="HEAD")
            if not filenames:
                return []
            return filenames.split("\n")
        except git.exc.GitCommandError:
            return []

    def is_file_tracked(self, filename: str) -> bool:
        """
        Args:
            filename (str): The absolute or relative file or folder path to check for.

        Returns:
            bool: Whether the file is tracked by Gitalong.
        """
        if self._is_ignored(filename):
            return False
        tracked_extensions = self.config.get("tracked_extensions", [])
        if os.path.splitext(filename)[-1] in tracked_extensions:
            return True
        # The binary check is expensive, so doing it last.
        # TODO: If we check for a file that does not exist locally if will return False.
        # That file technically could be a tracked binary modifed somewhere else.
        return self.config.get("track_binaries", False) and is_binary_file(
            self.get_absolute_path(filename), safe=True
        )

    @property
    def working_dir(self) -> str:
        """
        Returns:
            str: The working directory of the managed repository.
        """
        return str(self._managed_repository.working_dir)

    def pulled_within(self, seconds: float) -> bool:
        """
        Args:
            seconds (float): Time in seconds since last push.

        Returns:
            bool: Whether the repository pulled within the time provided.
        """
        return pulled_within(self._managed_repository, seconds)

    def log(self, message: str):
        """Logs a message to the managed repository.

        Args:
            message (str): The message to log.
        """
        self._managed_repository.git.log(message)

    @property
    def git(self) -> git.Git:
        """
        Returns:
            git.cmd.Git: The Git command line interface for the managed repository.
        """
        return self._managed_repository.git

    @property
    def batch(self):
        """
        Returns:
            Batch: The batch object for the managed repository.
        """
        from . import batch  # pylint: disable=import-outside-toplevel

        return batch

    @property
    def active_branch_name(self) -> str:
        """
        Returns:
            str: The name of the active branch.
        """
        return self._managed_repository.active_branch.name
