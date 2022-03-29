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
from .exceptions import RepositoryNotSetup
from .functions import set_read_only, pulled_within, get_filenames_from_move_string


class Repository:
    """The Gitalong class aggregates all the Gitalong actions that can happen on a
    repository.
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
            GitalongNotInstalled: Description
        """
        self._config = None
        self._submodules = None

        self._managed_repository = Repo(repository, search_parent_directories=True)
        self._store_repository = self._clone_store_repository()

        if self.config.get(
            "modify_permissions", False
        ) and self._managed_repository.config_reader().get_value(
            "core", "fileMode", True
        ):
            config_writer = self._managed_repository.config_writer()
            config_writer.set_value("core", "fileMode", "false")
            config_writer.release()

    def _clone_store_repository(self):
        """
        Returns:
            git.Repo: Clones the Gitalong repository if not done already.
        """
        try:
            return Repo(os.path.join(self.working_dir, ".gitalong"))
        except (git.exc.NoSuchPathError, git.exc.InvalidGitRepositoryError):
            remote = self.config.get("store_url")
            return Repo.clone_from(
                remote,
                os.path.join(self.working_dir, ".gitalong"),
            )

    @classmethod
    def setup(
        cls,
        store_repository: str,
        managed_repository: str = "",
        modify_permissions=False,
        pull_treshold: float = 60.0,
        track_binaries: bool = False,
        track_uncommitted: bool = False,
        tracked_extensions: list = None,
        update_gitignore: bool = False,
        update_hooks: bool = False,
    ):
        """Setup Gitalong in a repository.

        Args:
            store_repository (str):
                The URL or path to the repository that Gitalong will use to store local
                changes.
            managed_repository (str, optional):
                The repository in which we install Gitalong. Defaults to current
                working directory. Current working directory if not passed.
            modify_permissions (bool, optional):
                Whether Gitalong should managed permissions of binary files.
            track_binaries (bool, optional):
                Track all binary files by automatically detecting them.
            track_uncommitted (bool, optional):
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

        Returns:
            Gitalong:
                The Gitalong management class corresponding to the repository in
                which we just installed.
        """
        tracked_extensions = tracked_extensions or []
        managed_repository = Repo(managed_repository, search_parent_directories=True)
        config_path = os.path.join(managed_repository.working_dir, cls._config_basename)
        with open(config_path, "w", encoding="utf8") as _config_file:
            config_settings = {
                "store_url": store_repository,
                "modify_permissions": modify_permissions,
                "track_binaries": track_binaries,
                "tracked_extensions": tracked_extensions,
                "pull_treshold": pull_treshold,
                "track_uncommitted": track_uncommitted,
            }
            dump = json.dumps(config_settings, indent=4, sort_keys=True)
            _config_file.write(dump)
        gitalong = cls(repository=managed_repository.working_dir)
        gitalong._clone_store_repository()
        if update_gitignore:
            gitalong.update_gitignore()
        if update_hooks:
            gitalong.install_hooks()
        return gitalong

    def update_gitignore(self):
        """Update the .gitignore of the managed repository with Gitalong directives.

        TODO: Improve update by considering what is already ignored.
        """
        gitignore_path = os.path.join(self.working_dir, ".gitignore")
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
    def config_path(self) -> str:
        """
        Returns:
            dict: The content of `.gitalong.json` as a dictionary.
        """
        return os.path.join(self.working_dir, self._config_basename)

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
        return os.path.normpath(os.path.join(self.working_dir, basename))

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
            filename = os.path.relpath(filename, self.working_dir)
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
        return os.path.join(self.working_dir, filename)

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
        track_uncommitted = self.config.get("track_uncommitted", False)
        for tracked_commit in tracked_commits:
            if (
                # We ignore uncommitted tracked commits if configuration says so.
                (not track_uncommitted and "sha" not in tracked_commit)
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
            pull_treshold = self.config.get("pull_treshold", 60)
            if not pulled_within(self._managed_repository, pull_treshold):
                try:
                    self._managed_repository.remote().fetch(prune=prune)
                except git.exc.GitCommandError:
                    pass

            # TODO: Maybe there is a way to get this information using pure Python.
            args = ["--all", "--remotes", '--pretty=format:"%H"', "--", filename]
            output = self._managed_repository.git.log(*args)
            file_commits = output.replace('"', "").split("\n") if output else []
            last_commit = (
                self.get_commit_dict(
                    git.objects.Commit(
                        self._managed_repository, hex_to_bin(file_commits[0])
                    )
                )
                if file_commits
                else {}
            )
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
                        CommitSpread.MINE_ACTIVE_BRANCH
                        if is_issued
                        else CommitSpread.THEIR_MATCHING_BRANCH
                    )
                else:
                    commit_spread |= (
                        CommitSpread.MINE_OTHER_BRANCH
                        if is_issued
                        else CommitSpread.THEIR_OTHER_BRANCH
                    )
            else:
                commit_spread |= (
                    CommitSpread.MINE_UNCOMMITTED
                    if is_issued
                    else CommitSpread.THEIR_UNCOMMITTED
                )
        else:
            remote_branches = commit.get("branches", {}).get("remote", [])
            if active_branch in remote_branches:
                commit_spread |= CommitSpread.REMOTE_MATCHING_BRANCH
            if active_branch in commit.get("branches", {}).get("local", []):
                commit_spread |= CommitSpread.MINE_ACTIVE_BRANCH
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
            "clone": get_real_path(self.working_dir),
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
        if self.config.get("track_uncommitted"):
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

    @property
    def submodules(self) -> list:
        """
        Returns:
            TYPE: A list of submodule relative filenames.
        """
        if self._submodules is None:
            self._submodules = [_.name for _ in self._managed_repository.submodules]
        return self._submodules

    def is_submodule_file(self, filename) -> bool:
        """
        Args:
            filename (TYPE): Description

        Returns:
            TYPE: Whether a an absolute or relative filename belongs to a submodule.
        """
        for submodule in self.submodules:
            if self.get_relative_path(filename).startswith(submodule):
                return True
        return False

    @property
    def tracked_commits_json_path(self):
        """
        Returns:
            TYPE: The path to the JSON file that tracks the local commits.
        """
        return os.path.join(self._store_repository.working_dir, "commits.json")

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
            # TODO: HEAD might not be safe here since user could checkout an earlier
            # commit.
            filenames = git_cmd.ls_tree(full_tree=True, name_only=True, r="HEAD")
            return filenames.split("\n")
        except git.exc.GitCommandError:
            return []

    @property
    def locally_changed_files(self) -> list:
        """
        Returns:
            list:
                The relative filenames that have been changed by local commits or
                uncommitted changes.
        """
        local_changes = set()
        for commit in self.local_only_commits:
            local_changes = local_changes.union(commit.get("changes", []))
        return local_changes

    def update_file_permissions(
        self, filename: str, locally_changed_files: list = None
    ) -> tuple:
        """Updates the permissions of a file based on whether or not it was locally
        changed.

        Args:
            filename (str): The relative or absolute filename to update permissions for.
            locally_changed_files (list, optional):
                For optimization sake you can pass the locally changed files if you
                already have them. Default will compute them.

        Returns:
            tuple: A tuple featuring the permission and the filename.
        """
        locally_changed_files = locally_changed_files or self.locally_changed_files
        if self.is_file_tracked(filename):
            read_only = self.get_relative_path(filename) not in locally_changed_files
            if set_read_only(
                self.get_absolute_path(filename),
                read_only=read_only,
                check_exists=False,
            ):
                return ("R" if read_only else "W", filename)
        return ()

    def is_file_tracked(self, filename: str) -> bool:
        """
        Args:
            filename (str): The absolute or relative file or folder path to check for.

        Returns:
            bool: Whether the file is tracked by Gitalong.
        """
        if self.is_ignored(filename):
            return False
        tracked_extensions = self.config.get("tracked_extensions", [])
        if os.path.splitext(filename)[-1] in tracked_extensions:
            return True
        # The binary check is expensive so we are doing it last.
        return self.config.get("track_binaries", False) and is_binary_file(
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
            self._store_repository.index.add(json_path)
            basename = os.path.basename(json_path)
            self._store_repository.index.commit(message=f"Update {basename}")
            self._store_repository.remote().push()

    @property
    def tracked_commits(self) -> typing.List[dict]:
        """
        Returns:
            typing.List[dict]:
                A list of commits that haven't been pushed to remote. Also includes
                commits representing uncommitted changes.
        """
        store_repository = self._store_repository
        remote = store_repository.remote()
        pull_treshold = self.config.get("pull_treshold", 60)
        if not pulled_within(store_repository, pull_treshold) and remote.refs:
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
            spread & CommitSpread.MINE_ACTIVE_BRANCH == CommitSpread.MINE_ACTIVE_BRANCH
        )
        is_uncommitted = (
            spread & CommitSpread.MINE_UNCOMMITTED == CommitSpread.MINE_UNCOMMITTED
        )
        missing_commit = {} if is_local_commit or is_uncommitted else last_commit
        if os.path.exists(filename):
            if not missing_commit:
                set_read_only(filename, missing_commit)
        return missing_commit

    @property
    def working_dir(self):
        return self._managed_repository.working_dir
