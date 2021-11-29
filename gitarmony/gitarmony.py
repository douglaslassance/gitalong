import os
import shutil
import logging
import typing
import json
import socket

import configparser
import git

from .functions import get_real_path
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
        self._managed_repository = git.Repo(
            managed_repository, search_parent_directories=True
        )
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
        managed_repository = git.Repo(
            managed_repository, search_parent_directories=True
        )
        gitarmony_repository = git.Repo.clone_from(
            gitarmony_repository,
            os.path.join(managed_repository.working_dir, ".gitarmony"),
        )
        managed_repository.create_submodule(".gitarmony", gitarmony_repository)
        gitarmony = cls(managed_repository.working_dir)
        gitarmony.update_gitignore()
        gitarmony.install_hooks()
        gitarmony.install_actions(platform=platform)

        config = gitarmony.config
        config["settings"] = {"origin": gitarmony_repository, "secret": secret}
        with open(gitarmony.config_path, "w") as _file:
            config.write(_file)

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
    def config_path(self):
        """
        Returns:
            TYPE: The path the config file stored in the managed repository.
        """
        return os.path.join(self._gitarmony_repository.working_dir, ".gitarmony")

    @property
    def config(self) -> dict:
        """
        Returns:
            dict: The content of `.gitarmony.cfg` as a dictionary.
        """
        config = configparser.ConfigParser()
        config_path = self.config_path
        if os.path.exists(config_path):
            config.read(config_path)
        return config

    def sync(self):
        """Synchronize the change set with origin."""
        self.change_set.sync()

    @property
    def hooks_path(self):
        """The hook path of the managed repository.

        Returns:
            TYPE: Description
        """
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

    def last_file_commit(self, filename: str) -> git.objects.Commit:
        """
        Args:
            filename (str): The absolute or relative filename to get the last commit for.

        Returns:
            git.objects.Commit:
                The last commit for the provided filename across all branches local or
                remote.
        """
        self._managed_repository.remotes.origin.fetch(prune=True)

        # TODO: git log --all --first-parent --remotes --reflog --author-date-order --oneline -- gitarmony/change_set.py
        file_commits = []

        real_path = get_real_path(filename)
        tracked_commits = self.get_tracked_commits()
        relevant_tracked_commits = []
        for tracked_commit in tracked_commits:
            for change in tracked_commit.get("changes", []):
                change = get_real_path(
                    os.path.join(self._managed_repository.working_dir, change)
                )
                if change == real_path:
                    relevant_tracked_commits.append(tracked_commit)
        file_commits += relevant_tracked_commits
        file_commits.sort(key=lambda commit: commit.get("date"))
        return file_commits[-1] if file_commits else None

    @property
    def active_branch_commits(self) -> list:
        """
        Returns:
            list: List of all local commits for active branch.
        """
        active_branch = self._managed_repository.active_branch
        return list(git.objects.commit.iter_items(active_branch, active_branch))

    def has_commit(self, commit: git.objects.Commit) -> bool:
        """
        Args:
            commit (TYPE): The commit to check for.

        Returns:
            bool: Whether the active branch has a specific commit.
        """
        return commit in self.active_branch_commits

    @property
    def local_commits(self) -> list:
        """
        Returns:
            list: Commits that are not on remote branches.
        """
        # TODO: Get local commits and create a fake commit for uncommitted changes.
        return []

    def get_commit_dict(self, commit: git.objects.Commit) -> dict:
        """
        Args:
            commit (TYPE): Description

        Returns:
            dict: Description
        """
        return {
            "sha": commit.binsha,
            "origin": self._managed_repository.remotes.origin.url,
            "host": socket.gethostname(),
            "user": os.getusername(),
            "clone": get_real_path(self._managed_repository.working_dir),
            "changes": [diff.b_path for diff in commit.diff()],
            "date": commit.date,
        }

    @property
    def json_path(self):
        """
        Returns:
            TYPE: The path to the JSON file that tracks the commits.
        """
        return os.path.join(self._gitarmony_repository.working_dir, "commits.json")

    def update_tracked_commits(self, push=True):
        """Updates the JSON that tracks commits with latest local commits.

        Args:
            push (bool, optional):
                Whether we should push the changes immediately to origin.
        """
        tracked_commits = set(self.get_tracked_commits())
        for commit in self.local_commits:
            tracked_commits.add(commit)
        json_path = self.json_path
        with open(json_path, "w") as _file:
            _file.write(json.dumps(tracked_commits))
        if push:
            self._gitarmony_repository.add(json_path)
            basename = os.path.basename(json_path)
            self._gitarmony_repository.index.commit(message=f":lock: Update {basename}")
            self._gitarmony_repository.remotes.origin.push()

    def get_tracked_commits(self, pull=True) -> typing.List[dict]:
        """
        Args:
            pull (bool, optional): Whether or not we want to pull the latest tracked commits.

        Returns:
            typing.List[dict]: The list of commits that are tracked.
        """
        if pull:
            self._gitarmony_repository.remotes.origin.pull(fast_forward=True)
        serializable_commits = []
        origin = self._managed_repository.remotes.origin.url
        with open(self.json_path, "r") as _file:
            serializable_commits = json.loads(_file.read())
        relevant_commits = []
        for commit in serializable_commits:
            if commit.get("origin") == origin:
                relevant_commits.append(commit)
        return relevant_commits

    def make_writable(self, filename: str) -> git.objects.Commit:
        """
        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the current working directory.

        Returns:
            git.objects.Commit: The conflicting commit that we are missing.
        """
        filename = os.path.abspath(filename)
        if not os.path.isfile(filename):
            logging.error("File does not exists.")
            return None
        last_file_commit = self.last_file_commit(filename)
        if not last_file_commit or self.has_commit(last_file_commit):
            set_file_read_only(filename, False)
            logging.info("Made {} writable with success!")
            return None
        logging.info("Could not make {} writable.")
        return last_file_commit
