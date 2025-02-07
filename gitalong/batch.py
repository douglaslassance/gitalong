import os
import stat
import asyncio

# import datetime

# from turtle import write
from typing import List, Coroutine, Optional

# from unittest import result

import git
import git.exc

# from more_itertools import last

from .enums import CommitSpread
from .repository import Repository
from .commit import Commit

# from .functions import is_binary_file
from .exceptions import CommandError


async def _run_command(args: List[str], safe: bool = False) -> str:
    """
    Args:
        args (List[str]): The command to run.
        safe (bool): If True, suppress exceptions and return an empty string on failure.

    Raises:
        Exception: When the command fails and safe is False.

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
        if safe:
            return ""
        error = stderr.decode().strip()
        raise CommandError(f"Command {' '.join(args)} failed with error: {error}")
    return stdout.decode().strip()


async def get_local_only_commits(
    repository: Repository, claims: Optional[List[str]] = None
) -> list:
    """
    Returns:
        list:
            Commits that are not on remote branches. Includes a commit that
            represents uncommitted changes.
        claims (list, optional):
            List of files that we want to claim.
            They'll be store as uncommitted changes.
    """
    local_commits = []
    for branch in repository.branches:
        await accumulate_local_only_commits(repository, branch.commit, local_commits)
    if repository.config.get("track_uncommitted"):
        uncommitted_changes_commit = repository.get_uncommitted_changes_commit(
            claims=claims
        )

        # Adding file we want to claim to the uncommitted changes commit.
        for claim in claims or []:
            claim = repository.get_absolute_path(claim)
            if os.path.isfile(claim):
                uncommitted_changes_commit.setdefault("changes", []).append(
                    repository.get_relative_path(claim).replace("\\", "/")
                )

        if uncommitted_changes_commit:
            local_commits.insert(0, uncommitted_changes_commit)
    local_commits.sort(key=lambda commit: commit.get("date"), reverse=True)
    return local_commits


async def accumulate_local_only_commits(
    repository, start: git.Commit, local_commits: List[dict]
):
    """Accumulates a list of local only commit starting from the provided commit.

    Args:
        local_commits (list): The accumulated local commits.
        start (git.objects.Commit):
            The commit that we start peeling from last commit.
    """
    if repository.is_remote_commit(start.hexsha):
        return

    commit = Commit(repository)
    commit.update_with_sha(start.hexsha)
    commit.update_context()

    changes = await get_commits_changes([commit])
    commit["changes"] = changes[0]

    branches_list = await get_commits_branches([commit])
    branches = branches_list[0] if branches_list else []
    commit["branches"] = {"local": branches}

    if commit not in local_commits:
        local_commits.append(commit)
    for parent in start.parents:
        await accumulate_local_only_commits(repository, parent, local_commits)


async def get_files_last_commits(  # pylint: disable=too-many-branches,too-many-locals,too-many-statements
    filenames: List[str], prune: bool = True
) -> List[Commit]:
    """Get the last commit for a list of files.

    Args:
        filenames (List[str]): A list of absolute filenames to get the last commit for.
        prune (bool): Prune branches if a fetch is necessary.

    Returns:
        List[Commit]: A list of last commits for the files.
    """
    pruned_repositories: List[str] = []
    last_commits: List[Commit] = []
    for filename in filenames:
        repository = Repository.from_filename(os.path.dirname(filename))
        last_commit = Commit(repository)

        if not repository:
            last_commits.append(last_commit)
            continue

        if not repository.is_file_tracked(filename):
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

            args = [
                "--all",
                "--remotes",
                '--pretty=format:"%H"',
                # TODO: It is said that the chronological order of commits in the commit
                # history does not necessarily reflect the order of their commit dates.
                "--date-order",
                "--",
                filename,
            ]
            output = repository.git.log(*args)
            file_commits = output.replace('"', "").split("\n") if output else []
            sha = file_commits[0] if file_commits else ""
            last_commit = Commit(repository)
            last_commit.update_with_sha(sha)

        last_commits.append(last_commit)

    changes = await get_commits_changes(last_commits)
    local_branches_list = await get_commits_branches(last_commits)
    remote_branches_list = await get_commits_branches(last_commits, remote=True)

    for last_commit, changes, local_branches, remote_branches in zip(
        last_commits, changes, local_branches_list, remote_branches_list
    ):
        if changes:
            last_commit["changes"] = changes
        if local_branches:
            branches = last_commit.setdefault("branches", {})
            branches["local"] = local_branches
        if remote_branches:
            branches = last_commit.setdefault("branches", {})
            branches["remote"] = remote_branches

    return last_commits


async def get_commits_branches(
    commits: List[Commit], remote: bool = False
) -> List[str]:
    """
    Args:
        commits (List[Commit]): The commits to check for branches.
        remote (bool, optional): Whether we should return local or remote branches.

    Returns:
        List[str]: A list of branch names that this commit is living on.
    """
    branches_list = []
    tasks = []
    for commit in commits:
        if "sha" not in commit:
            # We are going to run a fake task to keep the order of the results.
            task = asyncio.sleep(0)
        else:
            args = ["git", "-C", commit.repository.working_dir, "branch"]
            if remote:
                args += ["--remote"]
            args += ["--contains", commit.get("sha", "")]
            task = _run_command(args, safe=True)
        tasks.append(task)
    results = await asyncio.gather(*tasks)
    for commit, result in zip(commits, results):
        if "sha" not in commit:
            branches_list.append([])
        else:
            branches = result.replace("*", "")
            branches = branches.replace(" ", "")
            branches = branches.split("\n") if branches else []
            branch_names = set()
            for branch in branches:
                branch_names.add(branch.split("/")[-1])
            branches_list.append(list(branch_names))
    return branches_list


async def get_commits_changes(commits: List[Commit]) -> List[str]:
    """
    Args:
        commits (List[Commit]): The commits to get the changes from.

    Returns:
        List[str]: A list of filenames that have changed in the commit.
    """
    changes_list = []
    tasks: List[Coroutine] = []
    for commit in commits:
        if "changes" in commit:
            changes_list.append(commit["changes"])
            continue

        repository = commit.repository
        if not repository:
            changes_list.append([])
            continue

        sha = commit.get("sha", "")
        if not sha:
            changes_list.append([])
            continue

        # If changes have not been collected we do that.
        if not commit.parents:
            # For the first commit, use git show.
            args = [
                "git",
                "-C",
                repository.working_dir,
                "show",
                "--pretty=format:",
                "--name-only",
                sha,
            ]
        else:
            # For subsequent commits, using git diff-tree.
            args = [
                "git",
                "-C",
                repository.working_dir,
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                sha,
            ]
        tasks.append(_run_command(args))
        changes_list.append((None))

    results = await asyncio.gather(*tasks)
    for result in results:
        changes = result.split("\n")
        changes = [change for change in changes if change]
        index = changes_list.index(None)
        changes_list[index] = changes
    return changes_list


async def update_files_permissions(filenames: List[str]):
    """Updates the permissions of a file weather or not they can be changed.

    Args:
        filename (str): The relative or absolute filename to update permissions for.
    """
    tasks = []
    last_commits = await get_files_last_commits(filenames)
    write_permissions = []
    for filename, last_commit in zip(filenames, last_commits):
        repository = Repository.from_filename(os.path.dirname(filename))
        if not repository:
            continue
        spread = last_commit.commit_spread
        if not spread:
            continue
        is_uncommitted = spread == CommitSpread.MINE_UNCOMMITTED
        is_local = spread == CommitSpread.MINE_ACTIVE_BRANCH
        write_permission = is_uncommitted or is_local
        write_permissions.append(write_permission)
        tasks.append(_set_write_permission(filename, write_permission))
    await asyncio.gather(*tasks)


async def _set_write_permission(
    filename: str, write_permission: bool, safe: bool = False
) -> bool:
    """Set the write permission of a file asynchronously.

    Args:
        filename (str): The path to the file.
        write_permission (bool): Whether to make the file writable.
        safe (bool): Won't raise an exceptions if the file doesn't exist or is
        not accessible.

    Returns:
        bool: Whether the file ends in the desired state.
    """
    try:
        current_permissions = os.stat(filename).st_mode
        if write_permission:
            os.chmod(filename, current_permissions | stat.S_IWRITE)
        else:
            os.chmod(filename, current_permissions & ~stat.S_IWRITE)
    except FileNotFoundError:
        if safe:
            # If the file doesn't exist we can't set it's permissions.
            return False
        raise
    except PermissionError:
        if safe:
            if write_permission:
                return False
            return True
        raise
    return True


async def get_updated_tracked_commits(
    repository: Repository, claims: Optional[List[str]] = None
) -> list:
    """
    Returns:
        list:
            Local commits for all clones with local commits and uncommitted changes
            from this clone.
    """
    tracked_commits = []
    for commit in repository.store.commits:
        remote = repository.remote.url
        is_other_remote = commit.get("remote") != remote
        if is_other_remote or not commit.is_issued_commit():
            tracked_commits.append(commit)
            continue

    for commit in await get_local_only_commits(repository, claims=claims):
        tracked_commits.append(commit)
    return tracked_commits


async def update_tracked_commits(
    repository: Repository, claims: Optional[List[str]] = None
):
    """Pulls the tracked commits from the store and updates them."""
    repository.store.commits = await get_updated_tracked_commits(
        repository, claims=claims
    )
    absolute_filenames = []
    if repository.config.get("modify_permissions"):
        print("Updating permissions")
        for filename in repository.files:
            absolute_filenames.append(repository.get_absolute_path(filename))
        await repository.batch.update_files_permissions(absolute_filenames)


async def claim_files(
    filenames: List[str],
    prune: bool = True,
) -> List[Commit]:
    """If the file is available for changes, temporarily communicates files as changed.
    By communicate we mean the file will be marked as a local change until the next
    update of the tracked commits. Also makes the files writable if the configured is
    set to affect permissions.

    Args:
        filename (str):
            A list of absolute filenames to claim.
        prune (bool, optional): Prune branches if a fetch is necessary.

    Returns:
        List[Commit]: The blocking commits for the file we want to claim.
    """
    blocking_commits = []
    last_commits = await get_files_last_commits(filenames, prune=prune)
    for last_commit in last_commits:
        spread = last_commit.commit_spread
        is_local_commit = (
            spread & CommitSpread.MINE_ACTIVE_BRANCH == CommitSpread.MINE_ACTIVE_BRANCH
        )
        is_uncommitted = (
            spread & CommitSpread.MINE_UNCOMMITTED == CommitSpread.MINE_UNCOMMITTED
        )
        blocking_commits.append(
            Commit(None) if is_local_commit or is_uncommitted else last_commit
        )

    # Collect all repositories and corresponding file updates.
    filenames_by_repository = {}
    for filename_ in filenames:
        repository = Repository.from_filename(filename_)
        filenames_by_repository.setdefault(repository, []).append(filename_)

    # Updating the tracked commits for each repository affected.
    for repository in filenames_by_repository:
        if repository:
            await update_tracked_commits(
                repository,
                claims=filenames_by_repository.get(repository, []),
            )
    return blocking_commits
