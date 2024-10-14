import os
import asyncio

from typing import List, Coroutine

import git
import git.exc
from pkg_resources import working_set

from .enums import CommitSpread
from .repository import Repository
from .functions import set_read_only


async def get_files_last_commits(
    filenames: List[str], prune: bool = True
) -> List[dict]:
    """Get the last commit for a list of files.

    Args:
        filenames (List[str]): A list of absolute filenames to get the last commit for.

    Returns:
        List[str]: A list of last commits for the files.
    """
    pruned_repositories = []
    last_commits: List[git.Commit | dict] = []
    for filename in filenames:
        last_commit = {}

        repository = Repository.from_filename(os.path.dirname(filename))
        if not repository:
            last_commits.append(last_commit)
            continue

        # We are checking the tracked commit first as they represented local changes.
        # They are in nature always more recent. If we find a relevant commit here we
        # can skip looking elsewhere.
        tracked_commits = repository.store.commits
        relevant_tracked_commits = []
        filename = repository.get_relative_path(filename)
        remote = repository.remote_url
        track_uncommitted = repository.config.get("track_uncommitted", False)
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
            # we are checking if this commit is on remote and modify it, so it's
            # conform to a remote commit.

            # TODO: We likely want to batch this part.
            branches_list = await get_commits_branches([last_commit], remote=True)
            if "sha" in last_commit and branches_list[0]:
                tracked_commits.remove(last_commit)
                repository.store.commits = tracked_commits
                for key in repository.context_dict:
                    if key in last_commit:
                        del last_commit[key]

        if not last_commit:
            pull_threshold = repository.config.get("pull_threshold", 60)
            if not repository.pulled_within(pull_threshold):
                try:
                    # We fetch only once since it's costly.
                    working_dir = repository.working_dir
                    if working_dir not in pruned_repositories:
                        repository.remote.fetch(prune=prune)
                        pruned_repositories.append(working_dir)
                except git.exc.GitCommandError:
                    pass

            args = ["--all", "--remotes", '--pretty=format:"%H"', "--", filename]
            output = repository.git.log(*args)
            file_commits = output.replace('"', "").split("\n") if output else []
            sha = file_commits[0] if file_commits else ""
            last_commit = repository.get_commit(sha) or {}

        last_commits.append(last_commit)

    last_commit_dicts = await get_commits_dicts(last_commits)
    local_branches_list = await get_commits_branches(last_commit_dicts)
    remote_branches_list = await get_commits_branches(last_commit_dicts, remote=True)

    for last_commit_dict, local_branches, remote_branches in zip(
        last_commit_dicts, local_branches_list, remote_branches_list
    ):
        if local_branches:
            branches = last_commit_dict.setdefault("branches", {})
            branches["local"] = local_branches
        if remote_branches:
            branches = last_commit_dict.setdefault("branches", {})
            branches["remote"] = remote_branches

    return last_commit_dicts


async def get_commits_branches(commits: List[dict], remote: bool = False) -> List[str]:
    """
    Args:
        sha (str): The sha of the commit to check for.
        remote (bool, optional): Whether we should return local or remote branches.

    Returns:
        list: A list of branch names that this commit is living on.
    """
    branches_list = []
    tasks = []
    for commit in commits:
        if "sha" not in commit:
            branches_list.append([])
            continue
        args = ["git", "-C", commit.get("clone", ""), "branch"]
        if remote:
            args += ["--remote"]
        args += ["--contains", commit.get("sha", "")]
        tasks.append(run_command(args))
    results = await asyncio.gather(*tasks)
    for result in results:
        branches = result.replace("*", "")
        branches = branches.replace(" ", "")
        branches = branches.split("\n") if branches else []
        branch_names = set()
        for branch in branches:
            branch_names.add(branch.split("/")[-1])
        branches_list.append(list(branch_names))
    return branches_list


async def run_command(args: List[str]) -> str:
    """
    Args:
        args (List[str]): The command to run.

    Raises:
        Exception: When the command fails.

    Returns:
        str: The stdout of the command.
    """
    process = await asyncio.create_subprocess_exec(
        *args,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        stdin=asyncio.subprocess.DEVNULL,
    )
    stdout, stderr = await process.communicate()
    if process.returncode != 0:
        message = f"Command failed with error: {stderr.decode().strip()}"
        raise Exception(message)  # pylint: disable=broad-exception-raised
    return stdout.decode().strip()


async def get_commits_changes(commits: List[git.Commit | dict]) -> List[str]:
    """
    Args:
        commits (git.objects.Commit | dict): The commit to get the changes from.

    Returns:
        List[str]: A list of filenames that have changed in the commit.
    """
    changes_list = []
    tasks: List[Coroutine] = []
    for commit in commits:
        if isinstance(commit, dict):
            changes_list.append(commit.get("changes", []))
            continue
        working_dir = commit.repo.working_dir
        if not commit.parents:
            # For the first commit, use git show.
            args = [
                "git",
                "-C",
                working_dir,
                "show",
                "--pretty=format:",
                "--name-only",
                commit.hexsha,
            ]
        else:
            # For subsequent commits, using git diff-tree.
            args = [
                "git",
                "-C",
                working_dir,
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                commit.hexsha,
            ]
        tasks.append(run_command(args))
        changes_list.append((None))

    results = await asyncio.gather(*tasks)
    for result in results:
        changes = result.split("\n")
        changes = [change for change in changes if change]
        index = changes_list.index(None)
        changes_list[index] = changes
    return changes_list


async def get_commits_dicts(commits: List[git.Commit | dict]) -> List[dict]:
    """Get commit information for a list of commits.

    Args:
        commits (List[git.Commit | dict]): A list of commits.

    Returns:
        List[dict]: A list of commit dictionaries.
    """
    changes_list = await get_commits_changes(commits)
    commit_dicts = []
    for commit, changes in zip(commits, changes_list):
        if isinstance(commit, dict):
            commit_dicts.append(commit)
            continue
        commit_dicts.append(
            {
                "sha": commit.hexsha,
                "remote": commit.repo.remote().url,
                "changes": changes,
                "date": str(commit.committed_datetime),
                "author": commit.author.name,
                "clone": commit.repo.working_dir,
            }
        )
    return commit_dicts


async def claim_files(
    filenames: List[str],
    prune: bool = True,
) -> List[dict]:
    """If the file is available for changes, temporarily communicates files as changed.
    By communicate we mean until the next update of the tracked commits.
    Also makes the files writable if the configured is set to affect permissions.

    Args:
        filename (str): The absolute filename to the file to claim.
        prune (bool, optional): Prune branches if a fetch is necessary.

    Returns:
        List[dict]: The commits that we are missing.
    """
    missing_commits = []
    claimables_by_repositoy = {}
    last_commits = await get_files_last_commits(filenames, prune=prune)
    for filename, last_commit in zip(filenames, last_commits):
        # Not sure if we should let it raise here, but not sure given the batch context.
        try:
            repository = Repository.from_filename(os.path.dirname(filename))
        except git.exc.NoSuchPathError:
            repository = None
        config = repository.config if repository else {}
        modify_permissions = config.get("modify_permissions")
        spread = repository.get_commit_spread(last_commit) if repository else 0
        is_local_commit = (
            spread & CommitSpread.MINE_ACTIVE_BRANCH == CommitSpread.MINE_ACTIVE_BRANCH
        )
        is_uncommitted = (
            spread & CommitSpread.MINE_UNCOMMITTED == CommitSpread.MINE_UNCOMMITTED
        )
        missing_commit = {} if is_local_commit or is_uncommitted else last_commit
        if os.path.exists(filename):
            if not missing_commit and modify_permissions:
                set_read_only(filename, bool(missing_commit))
                claimables_by_repositoy.setdefault(repository, []).append(filename)
        missing_commits.append(missing_commit)
    return missing_commits
