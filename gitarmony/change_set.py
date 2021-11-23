import json
import logging
import os
import socket
import subprocess

import git

from .functions import (
    get_local_branches,
    get_remote_branches,
    is_binary_file,
    is_ci_runner_host,
    set_file_read_only,
    get_default_branch,
)


class ChangeSet(set):
    def __init__(self, managed_repository: git.Repo, data_repository: git.Repo):
        """
        Args:
            managed_repository (git.Repo): The repository that is managed.
            data_repository (git.Repo): The repository in which we store data.
        """
        set.__init__(self)
        self._managed_repository = managed_repository
        self._data_repository = data_repository
        self._json_path = os.path.join(
            str(self._data_repository.working_dir), "changes.json"
        )
        self._user = self._managed_repository.config_reader().get_value("user", "name")
        self._email = self._managed_repository.config_reader().get_value(
            "user", "email"
        )
        self._host = socket.gethostname()
        self._default_branch = get_default_branch(self._managed_repository)
        self.populate()

    def is_issued_change(self, change: dict):
        """TODO: This surely can be optimized!

        Args:
            lock (dict): The lock that need to be analyzed for relevance.

        Returns:
            bool: Whether the lock is relevant to the current repository clone.
        """
        remote_url = self._managed_repository.remote(name="origin").url
        if is_ci_runner_host():
            # Locks issued by the runner are meant to represent branch locks and
            # therefore won't have a username key.
            return change.get("origin") == remote_url and "username" not in change
        return (
            change.get("remote") == remote_url
            and change.get("user") == self._user
            and change.get("host") == self._host
            and change.get("clone") == self._managed_repository.working_dir
        )

    def get_changes(self, binary_only=False) -> set:
        """TODO: Do not use diff but instead get all the commits that the matching
        branch does not have and add the uncommitted changes.

        Command for commits:
            gitlog --oneline --no-decorate --no-abbrev-commit origin/main..origin/branch

        Command for commit diff:
            git diff COMMIT~ COMMIT --name-status

        Args:
            binary_only (bool, optional): Only return binary file changes.

        Returns: set: Return a set of serialized changes between this local branch and
        it's matching on remote. On a runner  it will be between the remote branch and
        the default branch of the repository.
        """
        cmd = "git -C {} remote update remote --prune".format(self.path)
        logging.debug(cmd)
        subprocess.run(cmd, capture_output=True)
        clone = str(self._managed_repository.working_dir)
        active_branch = self._managed_repository.active_branch.name
        # TODO: Would be nice to support branch of branches.
        diff_branch = (
            f"origin/{self._default_branch}"
            if is_ci_runner_host()
            else f"origin/{active_branch}"
        )
        cmd = f"git -C {clone} diff --name-status {diff_branch}"
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
                        os.path.join(clone, filename)
                    ):
                        continue
                # Just catching from files that may not exist or have weird permissions.
                except PermissionError as error:
                    logging.error(error)
                    continue
            change = {
                "filename": filename,
                "status": status,
                "origin": self._managed_repository.remote(name="origin").url,
                "branch": active_branch,
            }
            if not is_ci_runner_host():
                change.update(
                    {
                        "hostname": self._host,
                        "username": self._user,
                        "clone": clone,
                    }
                )
            changes.add(change)
        logging.debug("changes = {}".format(changes))
        return changes

    def populate(self):
        if not os.path.exists(self._json_path):
            return
        changes = self.get_changes(binary_only=True)
        clone = str(self._managed_repository.working_dir)

        self.update(changes)
        for change in changes:
            filename = os.path.join(clone, change.get("filename", ""))
            set_file_read_only(filename, False)
        # Filter out locks that aren't relevant to the new set of changes.
        dropped_changes = []
        active_branch = self._managed_repository.active_branch.name
        for change in self:
            if change not in changes:
                if (
                    self.is_issued_change(change)
                    and change.get("branch") == active_branch
                ):
                    filename = os.path.join(clone, change.get("filename", ""))
                    set_file_read_only(filename, True)
                    dropped_changes.append(change)
        for dropped_change in dropped_changes:
            self.remove(dropped_change)
        logging.info("Cleaning deleted branches locks.")
        branches = (
            get_remote_branches(self._managed_repository)
            if is_ci_runner_host()
            else get_local_branches(self._managed_repository)
        )
        deleted_branches_changes = []
        for change in self:
            if not self.is_issued_change(change) or change.get("branch") in branches:
                continue
            deleted_branches_changes.append(change)
        for change in deleted_branches_changes:
            if change in self:
                self.remove(change)

    def write(self):
        with open(self._json_path, "w") as fle:
            logging.debug("changes={}".format(self))
            json_string = json.dumps(list(self), indent=4, sort_keys=True)
            fle.write(json_string)
            logging.info("Change list was written successfully!")

    def synchronize(self):
        self.write()
        self._data_repository.index.commit(message=":lock: Update {basename}")
        self._data_repository.remote(name="origin").push()
        logging.info("Change list was synchronized successfully!")
