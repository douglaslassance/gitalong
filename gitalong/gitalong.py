import os
import shutil
import logging
import typing
import json
import socket
import datetime
import getpass
import configparser

import dictdiffer
import git

from git.repo import Repo
from gitdb.util import hex_to_bin

from .enums import CommitSpread
from .functions import get_real_path, is_binary_file
from .exceptions import GitalongNotInstalled
from .functions import set_read_only, pulled_within, get_filenames_from_move_string


class Gitalong:
    """The Gitalong class aggregates all the Gitalong actions that can happen on a
    repository.

    Attributes:
        config_basename (str): The basename of the Gitalong configuration file.
    """

    config_basename = ".gitalong.json"

    def __init__(self, managed_repository: str = "", git_binary: str = ""):
        """
        Args:
            managed_repository (str, optional):
                The managed repository. Current working directory if not passed.
            git_binary (str, optional):
                Path to specific Git binary. Will find the one in PATH by default.

        Raises:
            GitalongNotInstalled: Description

        No Longer Raises:
            git.exc.GitalongNotInstalled: Description
            gitalong.exceptions.GitalongNotInstalled: Description
        """
        if git_binary:
            git.refresh(git_binary)
        self._managed_repository = Repo(
            managed_repository, search_parent_directories=True
        )
        try:
            with open(self.config_path, encoding="utf8") as _config_file:
                self._config = json.loads(_config_file.read())
        except FileNotFoundError as error:
            raise GitalongNotInstalled(
                "Gitalong is not installed on this repository."
            ) from error

        if self._config.get(
            "modify_permissions", False
        ) and self._managed_repository.config_reader().get_value(
            "core", "fileMode", True
        ):
            config_writer = self._managed_repository.config_writer()
            config_writer.set_value("core", "fileMode", "false")
            config_writer.release()
        self._gitalong_repository = self._clone_gitalong_repository()

    def _clone_gitalong_repository(self):
        """
        Returns:
            git.Repo: Clones the gitalong repository if not done already.
        """
        try:
            return Repo(os.path.join(self.managed_repository_root, ".gitalong"))
        except (git.exc.NoSuchPathError, git.exc.InvalidGitRepositoryError):
            remote = self._config.get("remote_url")
            return Repo.clone_from(
                remote,
                os.path.join(self.managed_repository_root, ".gitalong"),
            )

    @classmethod
    def install(
        cls,
        gitalong_repository: str,
        managed_repository: str = "",
        modify_permissions=False,
        pull_treshold: float = 60.0,
        track_binaries: bool = False,
        track_uncomitted: bool = False,
        tracked_extensions: dict = None,
        update_gitignore: bool = False,
        update_hooks: bool = False,
        git_binary: str = "",
    ):
        """Install Gitalong on a repository.

        Args:
            gitalong_repository (str):
                The URL of the repository that will store gitalong data.
            managed_repository (str, optional):
                The repository in which we install Gitalong. Defaults to current
                working directory. Current working directory if not passed.
            modify_permissions (bool, optional):
                Whether Gitalong should managed permissions of binary files.
            track_binaries (bool, optional):
                Track all binary files by automatically detecting them.
            track_uncomitted (bool, optional):
                Track uncommitted changes. Better for collaboration but requires to push
                tracked commits after each file system operation.
            tracked_extensions (list, optional):
                List of extensions to track.
            pull_treshold (list, optional):
                Time in seconds that need to pass before Gitalong pulls again. Defaults
                to 10 seconds. This is for optimization sake as pull and fetch operation
                are expensive. Defaults to 60 seconds.
            update_gitignore (bool, optional):
                Whether .gitignore should be modified in the managed repository to
                ignore Gitalong files.
            update_hooks (bool, optional):
                Whether hooks should be updated with Gitalong logic.
            git_binary (str, optional):
                Path to specific Git binary. Will find the one in PATH by default.

        Deleted Parameters:
            Returns:
                Gitalong:
                    The gitalong management class corresponding to the repository in
                    which we just installed.
        """
        tracked_extensions = tracked_extensions or []
        managed_repository = Repo(managed_repository, search_parent_directories=True)
        config_path = os.path.join(managed_repository.working_dir, cls.config_basename)
        with open(config_path, "w", encoding="utf8") as _config_file:
            config_settings = {
                "remote_url": gitalong_repository,
                "modify_permissions": modify_permissions,
                "track_binaries": track_binaries,
                "tracked_extensions": ",".join(tracked_extensions),
                "pull_treshold": pull_treshold,
                "track_uncomitted": track_uncomitted,
            }
            dump = json.dumps(config_settings, indent=4, sort_keys=True)
            _config_file.write(dump)
        gitalong = cls(
            managed_repository=managed_repository.working_dir, git_binary=git_binary
        )
        gitalong._clone_gitalong_repository()
        if update_gitignore:
            gitalong.update_gitignore()
        if update_hooks:
            gitalong.install_hooks()
        return gitalong

    def update_gitignore(self):
        """Update the .gitignore of the managed repository with Gitalong directives.

        TODO: Improve update by considering what is already ignored.
        """
        gitignore_path = os.path.join(self.managed_repository_root, ".gitignore")
        content = ""
        if os.path.exists(gitignore_path):
            with open(gitignore_path, encoding="utf8") as gitignore:
                content = gitignore.read()
        with open(gitignore_path, "w", encoding="utf8") as gitignore:
            with open(
                # Reading our .gitignore template.
                os.path.join(os.path.dirname(__file__), "resources", "gitignore"),
                encoding="utf8",
            ) as patch:
                patch_content = patch.read()
            if patch_content not in content:
                gitignore.write(content + patch_content)

    @property
    def managed_repository(self) -> Repo:
        """
        Returns:
            git.Repo: The repository we are managing with Gitalong.
        """
        return self._managed_repository

    @property
    def config_path(self) -> str:
        """
        Returns:
            dict: The content of `.gitalong.json` as a dictionary.
        """
        return os.path.join(self.managed_repository_root, self.config_basename)

    @property
    def config(self) -> dict:
        """
        Returns:
            dict: The content of `.gitalong.json` as a dictionary.
        """
        return self._config

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
        return os.path.normpath(os.path.join(self.managed_repository_root, basename))

    def install_hooks(self):
        """Installs Gitalong hooks in managed repository.

        TODO: Implement non-destructive version of these hooks. Currently we don't have
        any consideration for preexisting content.
        """
        hooks = os.path.join(os.path.dirname(__file__), "resources", "hooks")
        destination_dir = self.hooks_path
        for (dirname, _, basenames) in os.walk(hooks):
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
            filename = os.path.relpath(filename, self.managed_repository_root)
        return filename

    def get_absolute_path(self, filename: str) -> str:
        """
        Args:
            filename (str): The path relative to the managed repository.

        Returns:
            str: The absolute path.
        """
        if os.path.exists(filename):
            return filename
        return os.path.join(self.managed_repository_root, filename)

    def get_file_last_commit(self, filename: str, prune: bool = True) -> dict:
        """
        Args:
            filename (str): Absolute or relative filename to get the last commit for.
            prune (bool, optional): Prune branches if a fetch is necessary.

        Returns:
            dict: The last commit for the provided filename across all branches local or
                remote.
        """
        # We are checking the tracked commit first as they represented local changes.
        # They are in nature always more recent. If we find a relevant commit here we
        # can skip looking elsewhere.
        tracked_commits = self.tracked_commits
        relevant_tracked_commits = []
        filename = self.get_relative_path(filename)
        remote = self._managed_repository.remote().url
        last_commit = {}
        track_uncomitted = self.config.get("track_uncomitted", False)
        for tracked_commit in tracked_commits:
            if (
                # We ignore uncommitted tracked commits if configuration says so.
                (not track_uncomitted and "sha" not in tracked_commit)
                # We ignore commits from other remotes.
                or tracked_commit.get("remote") != remote
            ):
                continue
            for change in tracked_commit.get("changes", []):
                if os.path.normpath(change) == os.path.normpath(filename):
                    relevant_tracked_commits.append(tracked_commit)
                    continue
        if relevant_tracked_commits:
            relevant_tracked_commits.sort(key=lambda commit: commit.get("date"))
            last_commit = relevant_tracked_commits[-1]
            # Because there is no post-push hook a local commit that got pushed could
            # have never been removed from our tracked commits. To cover for this case
            # we are checking if this commit is on remote and modify it so it's
            # conform to a remote commit.
            if "sha" in last_commit and self.get_commit_branches(
                last_commit["sha"], remote=True
            ):
                tracked_commits.remove(last_commit)
                self.update_tracked_commits(tracked_commits)
                for key in self.context_dict:
                    if key in last_commit:
                        del last_commit[key]
        if not last_commit:
            pull_treshold = self._config.get("pull_treshold", 10)
            if not pulled_within(self._managed_repository, pull_treshold):
                try:
                    self._managed_repository.remote().fetch(prune=prune)
                except git.exc.GitCommandError:
                    pass

            # TODO: Maybe there is a way to get this information using pure Python.
            args = ["--all", "--remotes", '--pretty=format:"%H"', "--", filename]
            output = self._managed_repository.git.log(*args)
            file_commits = output.replace('"', "").split("\n") if output else []
            file_commits = [
                self.get_commit_dict(
                    git.objects.Commit(self._managed_repository, hex_to_bin(c))
                )
                for c in file_commits
            ]
            file_commits.sort(key=lambda commit: commit.get("date"))
            last_commit = file_commits[-1] if file_commits else {}

            if last_commit and "sha" in last_commit:
                # We are only evaluating branch information here because it's expensive.
                last_commit["branches"] = {
                    "local": self.get_commit_branches(last_commit["sha"]),
                    "remote": self.get_commit_branches(last_commit["sha"], remote=True),
                }
        return last_commit

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

    def get_commit_spread(self, commit: dict) -> dict:
        """
        Args:
            commit (dict): The commit to check for.

        Returns:
            dict:
                A dictionary of commit spread information containing all
                information about where this commit lives across branches and clones.
        """
        commit_spread = 0
        active_branch = self._managed_repository.active_branch.name
        if commit.get("user", ""):
            is_issued = self.is_issued_commit(commit)
            if "sha" in commit:
                if active_branch in commit.get("branches", {}).get("local", []):
                    commit_spread |= (
                        CommitSpread.LOCAL_ACTIVE_BRANCH
                        if is_issued
                        else CommitSpread.CLONE_MATCHING_BRANCH
                    )
                else:
                    commit_spread |= (
                        CommitSpread.LOCAL_OTHER_BRANCH
                        if is_issued
                        else CommitSpread.CLONE_OTHER_BRANCH
                    )
            else:
                commit_spread |= (
                    CommitSpread.LOCAL_UNCOMMITTED
                    if is_issued
                    else CommitSpread.CLONE_UNCOMMITTED
                )
        else:
            remote_branches = commit.get("branches", {}).get("remote", [])
            if active_branch in remote_branches:
                commit_spread |= CommitSpread.REMOTE_MATCHING_BRANCH
            if active_branch in commit.get("branches", {}).get("local", []):
                commit_spread |= CommitSpread.LOCAL_ACTIVE_BRANCH
            if active_branch in remote_branches:
                remote_branches.remove(active_branch)
            if remote_branches:
                commit_spread |= CommitSpread.REMOTE_OTHER_BRANCH
        return commit_spread

    @staticmethod
    def is_uncommitted_changes_commit(commit: dict) -> bool:
        """
        Args:
            commit (dict): The commit dictionary.

        Returns:
            bool: Whether the commit dictionary represents uncommitted changes.
        """
        return "user" in commit.keys()

    @property
    def uncommitted_changes_commit(self) -> dict:
        """
        Returns:
            dict: Returns a commit dictionary representing uncommitted changes.
        """
        uncommitted_changes = self.uncommitted_changes
        if not uncommitted_changes:
            return {}
        commit = {
            "remote": self._managed_repository.remote().url,
            "changes": self.uncommitted_changes,
            "date": str(datetime.datetime.now()),
        }
        commit.update(self.context_dict)
        return commit

    def is_issued_commit(self, commit: dict) -> bool:
        """
        Args:
            commit (dict): The commit dictionary to check for.

        Returns:
            bool: Whether the commit was issued by the current context.
        """
        context_dict = self.context_dict
        diff_keys = set()
        for diff in dictdiffer.diff(context_dict, commit):
            if diff[0] == "change":
                diff_keys.add(diff[1])
            elif diff[0] in ("add", "remove"):
                diff_keys = diff_keys.union([key[0] for key in diff[2]])
        intersection = set(context_dict.keys()).intersection(diff_keys)
        return not intersection

    def is_issued_uncommitted_changes_commit(self, commit: dict) -> bool:
        """
        Args:
            commit (dict): Description

        Returns:
            bool:
                Whether the commit represents uncommitted changes and is issued by the
                current context.
        """
        if not self.is_uncommitted_changes_commit(commit):
            return False
        return self.is_issued_commit(commit)

    def accumulate_local_only_commits(
        self, start: git.objects.Commit, local_commits: list
    ):
        """Accumulates a list of local only commit starting from the provided commit.

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
            self.accumulate_local_only_commits(parent, local_commits)

    @property
    def context_dict(self) -> dict:
        """
        Returns:
            dict: A dict of contextual values that we attached to tracked commits.
        """
        return {
            "host": socket.gethostname(),
            "user": getpass.getuser(),
            "clone": get_real_path(self.managed_repository_root),
        }

    @property
    def local_only_commits(self) -> list:
        """
        Returns:
            list:
                Commits that are not on remote branches. Includes a commit that
                represents uncommitted changes.
        """
        local_commits = []
        # We are collecting local commit for all local branches.
        for branch in self._managed_repository.branches:
            self.accumulate_local_only_commits(branch.commit, local_commits)
        if self.config.get("track_uncomitted"):
            uncommitted_changes_commit = self.uncommitted_changes_commit
            if uncommitted_changes_commit:
                local_commits.insert(0, uncommitted_changes_commit)
        local_commits.sort(key=lambda commit: commit.get("date"), reverse=True)
        return local_commits

    @property
    def uncommitted_changes(self) -> list:
        """
        Returns:
            list: A list of unique relative filenames that feature uncommitted changes.
        """
        # TODO: Maybe there is a way to get this information using pure Python.
        git_cmd = self._managed_repository.git
        output = git_cmd.ls_files("--exclude-standard", "--others")
        untracked_changes = output.split("\n") if output else []
        output = git_cmd.diff("--cached", "--name-only")
        staged_changes = output.split("\n") if output else []
        # A file can be in both in untracked and staged changes. The set fixes that.
        return list(set(untracked_changes + staged_changes))

    def get_commit_dict(self, commit: git.objects.Commit) -> dict:
        """
        Args:
            commit (git.objects.Commit): The commit to get as a dict.

        Returns:
            dict: A simplified JSON serializable dict that represents the commit.
        """
        changes = []
        for change in list(commit.stats.files.keys()):
            changes += get_filenames_from_move_string(change)
        return {
            "sha": commit.hexsha,
            "remote": self._managed_repository.remote().url,
            "changes": changes,
            "date": str(commit.committed_datetime),
            "author": commit.author.name,
        }

    def get_commit_branches(self, hexsha: str, remote: bool = False) -> list:
        """
        Args:
            hexsha (str): The hexsha of the commit to check for.
            remote (bool, optional): Whether we should return local or remote branches.

        Returns:
            list: A list of branch names that this commit is living on.
        """
        args = ["--remote" if remote else []]
        args += ["--contains", hexsha]
        branches = self._managed_repository.git.branch(*args)
        branches = branches.replace("*", "")
        branches = branches.replace(" ", "")
        branches = branches.split("\n") if branches else []
        branch_names = set()
        for branch in branches:
            branch_names.add(branch.split("/")[-1])
        return list(branch_names)

    @property
    def tracked_commits_json_path(self):
        """
        Returns:
            TYPE: The path to the JSON file that tracks the local commits.
        """
        return os.path.join(self._gitalong_repository.working_dir, "commits.json")

    def update_tracked_files_permissions(self, force=False):
        """Updates binary permissions of tracked files."""
        if force and self._config.get("modify_permissions", False):
            self.make_tracked_files_read_only()

    def is_ignored(self, filename: str) -> bool:
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

    def make_tracked_files_read_only(self, dirname=""):
        """Make tracked files read-only. File changed locally will be skipped.

        Args:
            dirname (str, optional):
                The directory recurse into. Defaults to the managed repository root.
        """
        # Filtering out files that have changed locally. No need to make them read-only.
        rel_local_changes = set()
        for commit in self.local_only_commits:
            rel_local_changes.union(commit.get("changes", []))
        abs_local_changes = []
        for change in rel_local_changes:
            abs_local_changes.append(self.get_absolute_path(change))
        dirname = dirname or self.managed_repository_root
        for basename in os.listdir(dirname):
            path = os.path.join(dirname, basename)
            if os.path.isdir(path):
                if basename == ".git" or self.is_ignored(path):
                    continue
                self.make_tracked_files_read_only(path)
            else:
                if path not in abs_local_changes and self.is_file_tracked(path):
                    set_read_only(path, read_only=True, check_exists=False)

    def is_file_tracked(self, filename: str) -> bool:
        """
        Args:
            filename (str): The absolute or relative file or folder path to check for.

        Returns:
            bool: Whether the file is tracked by Gitalong.
        """
        if self.is_ignored(filename):
            return False
        tracked_extensions = self._config.get("tracked_extensions", [])
        if os.path.splitext(filename)[-1] in tracked_extensions:
            return True
        # The binary check is expensive so we are doing it last.
        return self._config.get("track_binaries", False) and is_binary_file(
            self.get_absolute_path(filename)
        )

    @property
    def updated_tracked_commits(self) -> list:
        """
        Returns:
            list:
                Local commits for all clones with local commits and uncommitted changes
                from this clone.
        """
        # Removing any matching contextual commits from tracked commits.
        # We are re-evaluating those.
        tracked_commits = []
        for commit in self.tracked_commits:
            remote = self._managed_repository.remote().url
            is_other_remote = commit.get("remote") != remote
            if is_other_remote or not self.is_issued_commit(commit):
                tracked_commits.append(commit)
                continue
        # Adding all local commit to the list of tracked commits.
        # Will include uncommitted changes as a "fake" commit.
        for commit in self.local_only_commits:
            tracked_commits.append(commit)
        return tracked_commits

    def update_tracked_commits(self, commits: list = None, push: bool = True):
        """Write and pushes JSON file that tracks the local commits from all clones
        using the passed commits.

        Args:
            commits (list, optional):
                The tracked commits to update with. Default to evaluating updated
                tracked commits.
            push (bool, optional):
                Whether we are pushing the update JSON file to the Gitalong repository
                remote.
        """
        commits = commits or self.updated_tracked_commits
        json_path = self.tracked_commits_json_path
        with open(json_path, "w") as _file:
            dump = json.dumps(commits, indent=4, sort_keys=True)
            _file.write(dump)
        if push:
            self._gitalong_repository.index.add(json_path)
            basename = os.path.basename(json_path)
            self._gitalong_repository.index.commit(message=f"Update {basename}")
            self._gitalong_repository.remote().push()

    @property
    def tracked_commits(self) -> typing.List[dict]:
        """
        Returns:
            typing.List[dict]:
                A list of commits that haven't been pushed to remote. Also includes
                commits representing uncommitted changes.
        """
        gitalong_repository = self._gitalong_repository
        remote = gitalong_repository.remote()
        pull_treshold = self._config.get("pull_treshold", 10)
        if not pulled_within(gitalong_repository, pull_treshold) and remote.refs:
            # TODO: If we could check that a pull is already happening then we could
            # avoid this try except and save time.
            try:
                remote.pull(
                    ff=True,
                    quiet=True,
                    rebase=True,
                    autostash=True,
                    verify=False,
                    summary=False,
                )
            except git.exc.GitCommandError:
                pass
        serializable_commits = []
        if os.path.exists(self.tracked_commits_json_path):
            with open(self.tracked_commits_json_path, "r") as _file:
                serializable_commits = json.loads(_file.read())
        return serializable_commits

    def make_file_writable(
        self,
        filename: str,
        prune: bool = True,
    ) -> dict:
        """Make a file writable if it's not missing with other tracked commits that
        aren't present locally.

        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the managed repository.

        Returns:
            dict: The missing commit that we are missing.
        """
        last_commit = self.get_file_last_commit(filename, prune=prune)
        spread = self.get_commit_spread(last_commit)
        is_local_commit = (
            spread & CommitSpread.LOCAL_ACTIVE_BRANCH
            == CommitSpread.LOCAL_ACTIVE_BRANCH
        )
        is_uncommitted = (
            spread & CommitSpread.LOCAL_UNCOMMITTED == CommitSpread.LOCAL_UNCOMMITTED
        )
        missing_commit = {} if is_local_commit or is_uncommitted else last_commit
        if not missing_commit:
            if os.path.exists(filename):
                set_read_only(filename, False)
        # Since we figured out this file should not be touched,we'll also lock the file
        # here in case it was not locked.
        if self._config.get("modify_permissions", False):
            if os.path.exists(filename):
                set_read_only(filename, True)
        return missing_commit

    @property
    def managed_repository_root(self) -> str:
        """
        Returns:
            str: The managed repository dirname.
        """
        return self._managed_repository.working_dir
