import json
import logging
import os
import socket
import subprocess

import dictdiffer
import git

from .functions import (
    get_local_branches,
    get_remote_branches,
    is_binary_file,
    is_ci_runner_host,
    set_file_read_only,
    get_default_branch,
    get_real_path,
)


class ChangeSet(set):
    def __init__(self, managed_repository: git.Repo, gitarmony_repository: git.Repo):
        """
        Args: managed_repository (git.Repo): The repository that is managed.
            gitarmony_repository (git.Repo): The repository in which we store Gitarmony
            data.
        """
        set.__init__(self)
        self._managed_repository = managed_repository
        self._gitarmony_repository = gitarmony_repository
        self._json_path = os.path.join(
            str(self._gitarmony_repository.working_dir), "changes.json"
        )
        self._user = self._managed_repository.config_reader().get_value("user", "name")
        self._email = self._managed_repository.config_reader().get_value(
            "user", "email"
        )
        self._host = socket.gethostname()
        self._default_branch = get_default_branch(self._managed_repository)
        self._clone = str(self._managed_repository.working_dir)
        self._origin = self._managed_repository.remote(name="origin").url
        self.update()

    def is_issued_change(self, change: dict, criterias: dict = None):
        """
        Args:
            change (dict): The change to check for.
            criterias (dict, optional):
                A list of key that are relevant to the issuing context.

        Returns:
            bool: Whether the change is originated from a context that matches the tests.

        Deleted Parameters:
            lock (dict): The change that need to be analyzed for relevance.
        """
        criterias = criterias or ["user", "host", "clone", "origin", "branch"]
        diffs = dictdiffer.diff(self.context, self.change)
        for diff in diffs:
            if diff[1] in criterias:
                return False
        return True

    def get_current_changes(self, binary_only=True) -> set:
        """TODO: Do not use diff but instead get all the commits that the matching
        branch does not have and add the uncommitted changes.

        Command for commits:
            gitlog --oneline --no-decorate --no-abbrev-commit origin/main..origin/branch

        Command for commit diff:
            git diff COMMIT~ COMMIT --name-status

        Args:
            binary_only (bool, optional): Only return binary file changes.

        Returns: set:
            Returns a set of serialized changes for this context. On local clone it will
            be changes between this local branch and the matching one on remote. On a
            runner, it will be the ones between the remote branch and the default one
            for the managed repository.
        """
        cmd = "git -C {} remote update remote --prune".format(self.path)
        logging.debug(cmd)
        subprocess.run(cmd, capture_output=True)
        active_branch = self._managed_repository.active_branch.name
        # TODO: Would be nice to support branch of branches.
        diff_branch = (
            f"origin/{self._default_branch}"
            if is_ci_runner_host()
            else f"origin/{active_branch}"
        )
        cmd = f"git -C {self._clone} diff --name-status {diff_branch}"
        logging.debug(cmd)
        output = subprocess.run(cmd, capture_output=True)
        diffs = output.stdout.decode("utf-8")
        diffs = diffs.split("\n") if diffs else []
        logging.debug("diffs = {}".format(diffs))
        changes = set()
        for diff in diffs:
            if not diff:
                continue
            status, filename = diff.split("\t")
            if binary_only:
                try:
                    if status == "D" or not is_binary_file(
                        os.path.join(self._clone, filename)
                    ):
                        continue
                # Just catching from files that may not exist or have weird permissions.
                except PermissionError as error:
                    logging.error(error)
                    continue

            change = self.context
            change.update(
                {
                    "filename": filename,
                    "status": status,
                }
            )
            if not is_ci_runner_host():
                change.update(
                    {
                        "host": self._host,
                        "user": self._user,
                        "clone": get_real_path(self._clone),
                    }
                )
            changes.add(change)
        logging.debug("changes = {}".format(changes))
        return changes

    def update(self):
        """Populate the set with the latest contextual changes as well as dropping the
        one that are not relevant anymore.
        """
        if not os.path.exists(self._json_path):
            return
        changes = self.get_current_changes(binary_only=True)
        clone = str(self._managed_repository.working_dir)

        # Update the set with all the changes found in this context.
        set.update(self, changes)
        for change in changes:
            filename = os.path.join(clone, change.get("filename", ""))
            set_file_read_only(filename, False)
        # Remove changes that where issued by this context but dropped from the set.
        dropped_changes = []
        active_branch = self._managed_repository.active_branch.name
        for change in self:
            if change not in changes:
                if self.is_issued_change(change):
                    filename = os.path.join(clone, change.get("filename", ""))
                    set_file_read_only(filename, True)
                    dropped_changes.append(change)
        for dropped_change in dropped_changes:
            self.remove(dropped_change)
        logging.info("Cleaning deleted branches locks.")
        existing_branches = (
            get_remote_branches(self._managed_repository)
            if is_ci_runner_host()
            else get_local_branches(self._managed_repository)
        )
        # Remove deleted branches changes from the set.
        deleted_branches_changes = []
        for change in self:
            criterias = ["user", "host", "clone", "origin"]
            if (
                self.is_issued_change(change, criterias=criterias)
                and change.get("branch") not in existing_branches
            ):
                deleted_branches_changes.append(change)
        for change in deleted_branches_changes:
            if change in self:
                self.remove(change)

    @property
    def json(self) -> str:
        """
        Returns:
            str: Change set as formatted JSON.
        """
        return json.dumps(list(self), indent=4, sort_keys=True)

    def write(self):
        """Writes the JSON to file."""
        with open(self._json_path, "w") as _file:
            logging.debug("changes={}".format(self))
            _file.write(self.json)
            logging.info("Change list was written successfully!")

    def sync(self):
        """Synchronize the change set with origin."""
        self.write()
        self._gitarmony_repository.index.commit(message=":lock: Update {basename}")
        self._gitarmony_repository.remote(name="origin").push()
        logging.info("Change list was synchronized successfully!")

    @property
    def context(self):
        active_branch = self._managed_repository.active_branch.name
        context = {"origin": self._origin, "branch": active_branch}
        if not self.is_ci_runner_host:
            context.update(
                {
                    "clone": self._clone,
                    "user": self._user,
                    "host": self._host,
                }
            )
        return context

    def is_conflicting_change(self, change: dict) -> bool:
        """
        Args:
            change (dict): The change that we want to check for.

        Returns:
            bool: True if the change conflicts with our context.
        """
        diffs = dictdiffer.diff(self.context, change)
        for diff in diffs:
            # If the change is for a different filename or origin it cannot be a
            # conflicting change.
            if diff[1] in ["filename", "origin"]:
                return False
            # From that point we know the filename or origin is the same.
            # If one of the following criteria is different, than we know we are conflicting.
            if diff[1] in ["user", "branch", "clone"]:
                return True
        return False

    def conflicting_changes(self, filename: str) -> list:
        """
        Args:
            filename (str):
                The file to make writable. Takes a path that's absolute or relative to
                the current working directory.

        Returns:
            dict:
                Changes that are conflicting with this filename in the current context.
        """
        filename = os.path.abspath(filename)
        if not os.path.isfile(filename):
            logging.error("File does not exists.")
            return
        conflicting_changes = []
        for change in self.change_set:
            if self.is_conflicting_change(change):
                conflicting_changes.append(change)
        return conflicting_changes
