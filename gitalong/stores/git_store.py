import os
import json
import typing
import git

from git.repo import Repo
from ..functions import pulled_within
from ..exceptions import RepositoryInvalidConfig
from ..store import Store


class GitStore(Store):
    """Implementation using a Git repository as storage."""

    def __init__(self, managed_repository):
        super().__init__(managed_repository)
        self._store_repository = self._clone()

    @property
    def commits(self) -> typing.List[dict]:
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
        if os.path.exists(self.json_path):
            with open(self.json_path, "r", encoding="utf-8") as _file:
                serializable_commits = json.loads(_file.read())
        return serializable_commits

    @property
    def json_path(self):
        """
        Returns:
            TYPE: The path to the JSON file that tracks the local commits.
        """
        return os.path.join(self._store_repository.working_dir, "commits.json")

    def _clone(self):
        """
        Returns:
            git.Repo: Clones the Gitalong repository if not done already.
        """
        try:
            return Repo(os.path.join(self._managed_repository.working_dir, ".gitalong"))
        except (git.exc.NoSuchPathError, git.exc.InvalidGitRepositoryError):
            try:
                remote = self._managed_repository.config["store_url"]
            except KeyError as error:
                config = self._managed_repository.config_path
                message = f'Could not find value for "store_url" in "{config}".'
                raise RepositoryInvalidConfig(message) from error
            return Repo.clone_from(
                remote,
                os.path.join(self._managed_repository.working_dir, ".gitalong"),
            )

    @commits.setter
    def commits(self, commits: typing.List[dict]):
        commits = commits or self.updated_tracked_commits
        json_path = self.json_path
        with open(json_path, "w", encoding="utf-8") as _file:
            dump = json.dumps(commits, indent=4, sort_keys=True)
            _file.write(dump)
        self._store_repository.index.add(json_path)
        basename = os.path.basename(json_path)
        self._store_repository.index.commit(message=f"Update {basename}")
        self._store_repository.remote().push()
