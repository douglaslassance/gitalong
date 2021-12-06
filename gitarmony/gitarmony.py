import os
import shutil
import logging
import typing
import glob
import json
import socket
import datetime
import getpass
import itertools

import dictdiffer
import configparser
import git

from gitdb.util import hex_to_bin

from .functions import get_real_path, is_binary_file
from .exceptions import GitarmonyNotInstalled

from .functions import set_read_only


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
        hooks: bool = True,
        set_binary_permissions=True,
    ):
        """
        Args:
            gitarmony_repository (str):
                The URL of the repository that will store gitarmony data.
            managed_repository (str, optional):
                The repository in which we install Gitarmony. Defaults to current
                working directory. Current working directory if not passed.
            hooks (bool, optional):
                Whether hooks should be installed.
            set_binary_permissions (bool, optional):
                Whether Gitarmony should managed permissions of binary files.

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
        gitarmony = cls(managed_repository.working_dir)
        gitarmony.update_gitignore()
        if hooks:
            gitarmony.install_hooks()

        config = gitarmony.config
        origin = gitarmony_repository.remotes.origin.url
        config["settings"] = {
            "origin": origin,
            "set_binary_permissions": int(set_binary_permissions),
        }
        with open(gitarmony.config_path, "w") as _file:
            config.write(_file)
        return gitarmony

    def update_gitignore(self):
        """Update the .gitignore of the managed repository with Gitarmony directives.

        TODO: Improve update by considering what is already ignored.
        """
        gitignore_path = os.path.join(
            self._managed_repository.working_dir, ".gitignore"
        )
        content = ""
        if os.path.exists(gitignore_path):
            with open(gitignore_path) as gitignore:
                content = gitignore.read()
        with open(gitignore_path, "w") as gitignore:
            with open(
                # Reading our .gitignore template.
                os.path.join(os.path.dirname(__file__), "resources", "gitignore")
            ) as patch:
                patch_content = patch.read()
            if patch_content not in content:
                gitignore.write(content + patch_content)

    @property
    def config_path(self):
        """
        Returns:
            TYPE: The absolute path to the config file stored in the managed repository.
        """
        return os.path.join(self._managed_repository.working_dir, ".gitarmony.cfg")

    @property
    def config(self) -> configparser.ConfigParser:
        """
        Returns:
            dict: The content of `.gitarmony.cfg` as a dictionary.
        """
        config = configparser.ConfigParser()
        config_path = self.config_path
        if os.path.exists(config_path):
            config.read(config_path)
        return config

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
        hooks = os.path.join(os.path.dirname(__file__), "resources", "hooks")
        destination_dir = self.hooks_path
        for (dirname, _, basenames) in os.walk(hooks):
            for basename in basenames:
                filename = os.path.join(dirname, basename)
                destination = os.path.join(destination_dir, basename)
                logging.info("Copying hook from {} to {}".format(filename, destination))
                shutil.copyfile(filename, destination)

    def get_relative_path(self, filename: str) -> str:
        if os.path.exists(filename):
            filename = os.path.relpath(filename, self._managed_repository.working_dir)
        return filename

    def get_absolute_path(self, filename: str) -> str:
        if os.path.exists(filename):
            return filename
        return os.path.join(self._managed_repository.working_dir, filename)

    def last_file_commit(self, filename: str, fetch: bool = True) -> dict:
        """
        Args:
            filename (str): The absolute or relative filename to get the last commit for.

        Returns:
            dict:
                The last commit for the provided filename across all branches local or
                remote.
        """
        if fetch:
            self._managed_repository.remotes.origin.fetch(prune=True)
        filename = self.get_relative_path(filename)
        args = [
            "--all",
            "--remotes",
            '--pretty=format:"%H"',
            "--",
            filename,
        ]
        # TODO: Maybe there is a way to get this information using pure Python.
        output = self._managed_repository.git.log(*args)
        file_commits = output.replace('"', "").split("\n") if output else []
        file_commits = [
            self.get_commit_dict(
                git.objects.Commit(self._managed_repository, hex_to_bin(c))
            )
            for c in file_commits
        ]
        tracked_commits = self.get_tracked_commits(pull=True)
        relevant_tracked_commits = []
        for tracked_commit in tracked_commits:
            for change in tracked_commit.get("changes", []):
                if os.path.normpath(change) == os.path.normpath(filename):
                    relevant_tracked_commits.append(tracked_commit)
        file_commits += relevant_tracked_commits
        file_commits.sort(key=lambda commit: commit.get("date"), reverse=True)
        return file_commits[-1] if file_commits else None

    @property
    def active_branch_commits(self) -> list:
        """
        Returns:
            list: List of all local commits for active branch.
        """
        active_branch = self._managed_repository.active_branch
        return list(
            git.objects.Commit.iter_items(self._managed_repository, active_branch)
        )

    def has_commit(self, commit: dict) -> bool:
        """
        Args:
            commit (dict): The commit to check for.

        Returns:
            bool: Whether the active branch has a specific commit.
        """
        hexsha = commit.get("sha", "")
        if not hexsha:
            return False
        commit = git.objects.Commit(self._managed_repository, hex_to_bin(hexsha))
        return commit in self.active_branch_commits

    def is_pending_changes_commit(self, commit: dict) -> bool:
        return "user" in commit.keys()

    @property
    def pending_changes_commit(self) -> dict:
        pending_changes = self.pending_changes
        if not pending_changes:
            return {}
        pending_changes_commit = {
            "origin": self._managed_repository.remotes.origin.url,
            "changes": self.pending_changes,
            "date": str(datetime.datetime.now()),
        }
        pending_changes_commit.update(self.context_dict)
        return pending_changes_commit

    def is_issued_commit(self, commit: dict) -> bool:
        context_dict = self.context_dict
        diff_types = [diff[-1] for diff in dictdiffer.diff(context_dict, commit)]
        diffs = list(itertools.chain.from_iterable(diff_types))
        diff_keys = {diff[0] for diff in diffs}
        intersection = set(context_dict.keys()).intersection(diff_keys)
        return not intersection

    def is_issued_pending_changes_commit(self, commit: dict) -> bool:
        if not self.is_pending_changes_commit(commit):
            return False
        return self.is_issued_commit(commit)

    def accumulate_local_commits(self, start: git.objects.Commit, local_commits: list):
        """Accumulates a list of local commit starting from the provided commit.

        Args:
            local_commits (list): The accumulated local commits.
            start (git.objects.Commit):
                The commit that we start peeling from last commit.
        """
        # TODO: Maybe there is a way to get this information using pure Python.
        if self._managed_repository.git.branch("--remotes", "--contains", start.hexsha):
            return
        commit_dict = self.get_commit_dict(start)
        commit_dict.update(self.context_dict)
        # TODO: Maybe we should compare the SHA here.
        if commit_dict not in local_commits:
            local_commits.append(commit_dict)
        for parent in start.parents:
            self.accumulate_local_commits(parent, local_commits)

    @property
    def context_dict(self) -> dict:
        """
        Returns:
            dict: A dict of contextual values that we attached to tracked commits.
        """
        return {
            "host": socket.gethostname(),
            "user": getpass.getuser(),
            "clone": get_real_path(self._managed_repository.working_dir),
        }

    @property
    def local_commits(self) -> list:
        """
        Returns:
            list: Commits and pending changes that are not on remote branches.
        """
        local_commits = []
        for branch in self._managed_repository.branches:
            self.accumulate_local_commits(branch.commit, local_commits)
        pending_changes_commit = self.pending_changes_commit
        if pending_changes_commit:
            local_commits.insert(0, pending_changes_commit)
        local_commits.sort(key=lambda commit: commit.get("date"), reverse=True)
        return local_commits

    @property
    def pending_changes(self) -> list:
        """
        Returns:
            list: A list of unique relative filename that are changed locally.
        """
        # TODO: Maybe there is a way to get this information using pure Python.
        git_cmd = self._managed_repository.git
        output = git_cmd.ls_files("--exclude-standard", "--others")
        untracked_changes = output.split("\n") if output else []
        output = git_cmd.diff("--cached", "--name-only")
        staged_changes = output.split("\n") if output else []
        # A file can be in both in un-tracked and staged changes. The set fixes that.
        return list(set(untracked_changes + staged_changes))

    def get_commit_dict(self, commit: git.objects.Commit) -> dict:
        """
        Args:
            commit (git.objects.Commit): The commit to get as a dict.

        Returns:
            dict: A simplified JSON serializable dict that represents the commit.
        """
        return {
            "sha": commit.hexsha,
            "origin": self._managed_repository.remotes.origin.url,
            "changes": list(commit.stats.files.keys()),
            "date": str(commit.committed_datetime),
            "author": commit.author.name,
        }

    @property
    def tracked_commits_json_path(self):
        """
        Returns:
            TYPE: The path to the JSON file that tracks the local commits.
        """
        return os.path.join(self._gitarmony_repository.working_dir, "commits.json")

    def sync(self):
        """Convenience method to get Gitarmony in sync on the local clone."""
        self.update_tracked_commits(push=True)
        update_binary_permisions = self.config["settings"].get(
            "update_binary_permisions", False
        )
        if update_binary_permisions:
            self.make_binary_files_read_only(self._managed_repository.working_dir)

    def is_ignored(self, filename: str) -> bool:
        """
        Args:
            filename (str): The filename to check for.

        Returns:
            bool: Whether if a file is ignored by the managed repository .gitignore file.
        """
        filename = self.get_relative_path(filename)
        try:
            self._managed_repository.git.check_ignore(filename)
            return True
        except git.exc.GitCommandError:
            return False

    def make_binary_files_read_only(self, dirname):
        """Make binary files that aren't ignored read-only."""
        for basename in os.listdir(dirname):
            filename = os.path.join(dirname, basename)
            if os.path.isdir(filename):
                if basename == '.git' or self.is_ignored(filename):
                    continue
                self.make_binary_files_read_only(filename)
            else:
                if self.is_ignored(filename) or not is_binary_file(filename):
                    continue
                set_read_only(filename, read_only=True, check_exists=False)

    def update_tracked_commits(self, push=True):
        """Updates the JSON that tracks local commits from everyone working on the
        repository by evaluating local commits and uncommitted changes.

        Args:
            push (bool, optional):
                Whether we should push the changes immediately to origin.
        """
        # Removing any matching contextual commits from tracked commits.
        # We are re-evaluating those.
        tracked_commits = []
        for commit in self.get_tracked_commits(pull=True):
            if not self.is_issued_commit(commit):
                tracked_commits.append(commit)
                continue
        # Adding all local commit to the list of tracked commits.
        # Will include uncommitted changes as a "fake" commit.
        for commit in self.local_commits:
            tracked_commits.append(commit)
        json_path = self.tracked_commits_json_path
        with open(json_path, "w") as _file:
            dump = json.dumps(tracked_commits, indent=4, sort_keys=True)
            _file.write(dump)
        if push:
            self._gitarmony_repository.index.add(json_path)
            basename = os.path.basename(json_path)
            self._gitarmony_repository.index.commit(message=f":lock: Update {basename}")
            self._gitarmony_repository.remote().push()

    def get_tracked_commits(self, pull=True) -> typing.List[dict]:
        """
        Args:
            pull (bool, optional):
                Whether or not we want to pull the latest tracked commits.

        Returns:
            typing.List[dict]:
                A list of tracked commits combining local commits and pending
                uncommitted changes.
        """
        origin = self._gitarmony_repository.remote()
        if pull and origin.refs:
            origin.pull(ff=True)
        serializable_commits = []
        origin = self._managed_repository.remotes.origin.url
        serializable_commits = []
        if os.path.exists(self.tracked_commits_json_path):
            with open(self.tracked_commits_json_path, "r") as _file:
                serializable_commits = json.loads(_file.read())
        relevant_commits = []
        for commit in serializable_commits:
            if commit.get("origin") == origin:
                relevant_commits.append(commit)
        return relevant_commits

    def get_conflicting_commit(self, filename: str) -> dict:
        """
        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the managed repository.
        Returns:
            dict:
                The latest conflicting commit that we are missing.
        """
        last_file_commit = self.last_file_commit(filename)
        if (
            not last_file_commit
            or self.has_commit(last_file_commit)
            or self.is_issued_commit(last_file_commit)
        ):
            return None
        return last_file_commit

    def make_writable(self, filename: str, force=False) -> dict:
        """Make a file writable if it's not conflicting with other tracked commits that
        aren't present locally.

        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the managed repository.
            force (bool, optional):
                Will make the file writable regardless of the status of the file. Kind
                of defeating the entire purpose of Gitarmony but provided for
                convenience.

        Returns:
            dict: The conflicting commit that we are missing.
        """
        conflicting_commit = self.get_conflicting_commit(filename)
        if force or not conflicting_commit:
            if os.path.exists(filename):
                set_read_only(filename, False)
        update_binary_permisions = self.config["settings"].get(
            "update_binary_permisions", False
        )
        # Since we figured out this file should not be touched,we'll also lock the file
        # here in case it was not locked.
        if update_binary_permisions:
            if os.path.exists(filename):
                set_read_only(filename, True)
        return conflicting_commit
