import os
import stat
import asyncio
import functools
from typing import List, Coroutine, Optional, Dict, Set, Tuple, Any, Union, cast
from functools import lru_cache

import git
import git.exc

from .enums import CommitSpread
from .repository import Repository
from .commit import Commit
from .exceptions import CommandError

# Constants
DEFAULT_PULL_THRESHOLD = 60
DEFAULT_TRACK_UNCOMMITTED = False
DEFAULT_MODIFY_PERMISSIONS = False


async def _run_command(args: List[str], safe: bool = False) -> str:
    """Run a command asynchronously and return its output.
    
    Args:
        args (List[str]): The command to run.
        safe (bool): If True, suppress exceptions and return an empty string on failure.

    Raises:
        CommandError: When the command fails and safe is False.

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
) -> List[Dict[str, Any]]:
    """Get commits that are not on remote branches including uncommitted changes.
    
    Args:
        repository: The repository to get local commits from.
        claims: List of files that we want to claim as uncommitted changes.
        
    Returns:
        List of commit dictionaries that are not on remote branches.
        Includes a commit that represents uncommitted changes.
    """
    # Collect local commits from all branches in parallel
    local_commits: List[Dict[str, Any]] = []
    tasks = [
        accumulate_local_only_commits(repository, branch.commit, local_commits)
        for branch in repository.branches
    ]
    await asyncio.gather(*tasks)
    
    # Add uncommitted changes if configured
    if repository.config.get("track_uncommitted", DEFAULT_TRACK_UNCOMMITTED):
        uncommitted_changes_commit = repository.get_uncommitted_changes_commit(
            claims=claims
        )

        # Add files we want to claim to the uncommitted changes commit
        if claims:
            for claim in claims:
                absolute_claim = repository.get_absolute_path(claim)
                if os.path.isfile(absolute_claim):
                    uncommitted_changes_commit.setdefault("changes", []).append(
                        repository.get_relative_path(absolute_claim).replace("\\", "/")
                    )

        if uncommitted_changes_commit:
            local_commits.insert(0, uncommitted_changes_commit)
            
    # Sort commits by date, newest first
    local_commits.sort(key=lambda commit: commit.get("date"), reverse=True)
    return local_commits


async def accumulate_local_only_commits(
    repository: Repository, start: git.Commit, local_commits: List[Dict[str, Any]]
) -> None:
    """Accumulate a list of local-only commits starting from the provided commit.

    Args:
        repository: The git repository.
        start: The commit that we start traversing from.
        local_commits: The accumulated local commits (modified in place).
    """
    # Skip if this is a remote commit
    if repository.is_remote_commit(start.hexsha):
        return

    # Create and initialize commit object
    commit = Commit(repository)
    commit.update_with_sha(start.hexsha)
    commit.update_context()

    # Get changes and branches in parallel
    changes_task = get_commits_changes([commit])
    branches_task = get_commits_branches([commit])
    changes_list, branches_list = await asyncio.gather(changes_task, branches_task)
    
    # Update commit with results
    commit["changes"] = changes_list[0]
    branches = branches_list[0] if branches_list else []
    commit["branches"] = {"local": branches}

    # Add to results if not already present
    if commit not in local_commits:
        local_commits.append(commit)
        
    # Process parents in parallel
    if start.parents:
        tasks = [
            accumulate_local_only_commits(repository, parent, local_commits)
            for parent in start.parents
        ]
        await asyncio.gather(*tasks)


async def _check_tracked_commits(
    repository: Repository, 
    rel_filename: str, 
    remote: str,
    track_uncommitted: bool
) -> Tuple[List[Dict[str, Any]], Optional[Dict[str, Any]]]:
    """Check if the file appears in tracked commits.
    
    Args:
        repository: The repository to check.
        rel_filename: Relative path of the file.
        remote: Remote URL to check against.
        track_uncommitted: Whether to consider uncommitted changes.
        
    Returns:
        Tuple of (filtered tracked commits, last relevant commit or None)
    """
    tracked_commits = repository.store.commits
    relevant_commits = []
    
    normalized_filename = os.path.normpath(rel_filename)
    
    # Find all commits that include this file
    for commit in tracked_commits:
        # Skip if conditions aren't met
        if ((not track_uncommitted and "sha" not in commit) or
                commit.get("remote") != remote):
            continue
            
        # Check if file is in changes
        for change in commit.get("changes", []):
            if os.path.normpath(change) == normalized_filename:
                relevant_commits.append(commit)
                break
                
    # Return the most recent commit if found
    if relevant_commits:
        relevant_commits.sort(key=lambda c: c.get("date"))
        return tracked_commits, relevant_commits[-1]
    
    return tracked_commits, None


async def _fetch_repository_if_needed(
    repository: Repository, 
    pruned_repositories: Set[str],
    prune: bool
) -> Set[str]:
    """Fetch from remote if needed.
    
    Args:
        repository: Repository to fetch.
        pruned_repositories: Set of already pruned repositories.
        prune: Whether to prune branches.
        
    Returns:
        Updated set of pruned repositories.
    """
    pull_threshold = repository.config.get("pull_threshold", DEFAULT_PULL_THRESHOLD)
    if not repository.pulled_within(pull_threshold):
        try:
            working_dir = repository.working_dir
            if working_dir not in pruned_repositories:
                repository.remote.fetch(prune=prune)
                pruned_repositories.add(working_dir)
        except git.exc.GitCommandError:
            pass
    
    return pruned_repositories


async def _get_commit_from_git_log(repository: Repository, rel_filename: str) -> Commit:
    """Get the last commit for a file from git log.
    
    Args:
        repository: The repository to query.
        rel_filename: Relative path to the file.
        
    Returns:
        Commit object with the last commit data.
    """
    args = [
        "--all",
        "--remotes",
        '--pretty=format:"%H"',
        "--date-order",
        "--",
        rel_filename,
    ]
    output = repository.git.log(*args)
    file_commits = output.replace('"', "").split("\n") if output else []
    sha = file_commits[0] if file_commits else ""
    
    last_commit = Commit(repository)
    if sha:
        last_commit.update_with_sha(sha)
    
    return last_commit


async def _process_file_last_commit(
    filename: str, 
    pruned_repositories: Set[str],
    prune: bool
) -> Tuple[Commit, Set[str]]:
    """Process a single file to find its last commit.
    
    Args:
        filename: Absolute path to the file.
        pruned_repositories: Set of already pruned repositories.
        prune: Whether to prune branches.
        
    Returns:
        Tuple of (commit object, updated set of pruned repositories)
    """
    # Initialize repository and basic commit
    repository = Repository.from_filename(os.path.dirname(filename))
    last_commit = Commit(repository)
    
    # Return empty commit for invalid cases
    if not repository or not repository.is_file_tracked(filename):
        return last_commit, pruned_repositories
    
    # Convert to relative path for git operations
    rel_filename = repository.get_relative_path(filename)
    remote = repository.remote_url
    track_uncommitted = repository.config.get("track_uncommitted", DEFAULT_TRACK_UNCOMMITTED)
    
    # Check tracked commits first (local changes)
    tracked_commits, commit_from_tracked = await _check_tracked_commits(
        repository, rel_filename, remote, track_uncommitted
    )
    
    if commit_from_tracked:
        last_commit = commit_from_tracked
        
        # Check if this commit is actually on remote
        branches_list = await get_commits_branches([last_commit], remote=True)
        if "sha" in last_commit and branches_list[0]:
            # Update tracked commits
            tracked_commits.remove(last_commit)
            repository.store.commits = tracked_commits
            # Remove context keys
            for key in repository.context_dict:
                if key in last_commit:
                    del last_commit[key]
    
    # If no commit found yet, try git log
    if not last_commit:
        # Fetch from remote if needed
        pruned_repositories = await _fetch_repository_if_needed(
            repository, pruned_repositories, prune
        )
        
        # Get commit from git log
        last_commit = await _get_commit_from_git_log(repository, rel_filename)
    
    return last_commit, pruned_repositories


async def get_files_last_commits(
    filenames: List[str], prune: bool = True
) -> List[Commit]:
    """Get the last commit for a list of files.

    Args:
        filenames: A list of absolute filenames to get the last commit for.
        prune: Prune branches if a fetch is necessary.

    Returns:
        A list of last commits for the files.
    """
    pruned_repositories: Set[str] = set()
    last_commits: List[Commit] = []
    
    # Process each file to get its last commit
    tasks = []
    for filename in filenames:
        task = _process_file_last_commit(filename, pruned_repositories, prune)
        tasks.append(task)
    
    # Execute file processing in parallel
    results = await asyncio.gather(*tasks)
    for last_commit, updated_pruned_repos in results:
        last_commits.append(last_commit)
        pruned_repositories.update(updated_pruned_repos)
    
    # Gather additional information in parallel
    changes_task = get_commits_changes(last_commits)
    local_branches_task = get_commits_branches(last_commits)
    remote_branches_task = get_commits_branches(last_commits, remote=True)
    
    changes, local_branches_list, remote_branches_list = await asyncio.gather(
        changes_task, local_branches_task, remote_branches_task
    )
    
    # Update commits with collected information
    for last_commit, changes_list, local_branches, remote_branches in zip(
        last_commits, changes, local_branches_list, remote_branches_list
    ):
        if changes_list:
            last_commit["changes"] = changes_list
        if local_branches:
            branches = last_commit.setdefault("branches", {})
            branches["local"] = local_branches
        if remote_branches:
            branches = last_commit.setdefault("branches", {})
            branches["remote"] = remote_branches

    return last_commits


async def _get_commit_branches(
    commit: Commit, 
    remote: bool = False
) -> List[str]:
    """Get branches for a single commit.
    
    Args:
        commit: The commit to check for branches.
        remote: Whether to check remote branches.
        
    Returns:
        List of branch names that this commit is on.
    """
    if "sha" not in commit:
        return []
        
    args = ["git", "-C", commit.repository.working_dir, "branch"]
    if remote:
        args += ["--remote"]
    args += ["--contains", commit.get("sha", "")]
    
    result = await _run_command(args, safe=True)
    
    if not result:
        return []
        
    # Process branch names
    branches = result.replace("*", "").replace(" ", "").split("\n")
    branch_names = {branch.split("/")[-1] for branch in branches if branch}
    
    return list(branch_names)


async def get_commits_branches(
    commits: List[Commit], 
    remote: bool = False
) -> List[List[str]]:
    """Get branches for multiple commits in parallel.
    
    Args:
        commits: The commits to check for branches.
        remote: Whether to check remote branches.
        
    Returns:
        A list of branch name lists that each commit is living on.
    """
    tasks = [
        _get_commit_branches(commit, remote) if "sha" in commit 
        else asyncio.sleep(0) and []  # Empty list for commits without SHA
        for commit in commits
    ]
    
    branches_list = await asyncio.gather(*tasks)
    return branches_list


async def _get_commit_changes(commit: Commit) -> List[str]:
    """Get changed files for a single commit.
    
    Args:
        commit: The commit to get changes for.
        
    Returns:
        List of filenames that changed in the commit.
    """
    # Return existing changes if available
    if "changes" in commit:
        return commit["changes"]
        
    # Return empty list for invalid cases
    repository = commit.repository
    if not repository:
        return []
        
    sha = commit.get("sha", "")
    if not sha:
        return []
        
    # Choose appropriate git command based on commit type
    if not commit.parents:
        # For the first commit, use git show
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
        # For subsequent commits, use git diff-tree
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
        
    # Execute command and process results
    result = await _run_command(args)
    return [change for change in result.split("\n") if change]


async def get_commits_changes(commits: List[Commit]) -> List[List[str]]:
    """Get changed files for multiple commits in parallel.
    
    Args:
        commits: The commits to get the changes from.
        
    Returns:
        A list of filename lists that have changed in each commit.
    """
    # Create tasks for commits that need changes fetched
    tasks = []
    result_mapping = {}  # Maps task to position in final result
    
    changes_list = []
    for i, commit in enumerate(commits):
        if "changes" in commit:
            changes_list.append(commit["changes"])
        elif not commit.repository or not commit.get("sha"):
            changes_list.append([])
        else:
            task = _get_commit_changes(commit)
            tasks.append(task)
            result_mapping[len(tasks) - 1] = i
            changes_list.append(None)  # Placeholder
            
    # Run tasks in parallel
    if tasks:
        results = await asyncio.gather(*tasks)
        
        # Update changes list with results
        for task_idx, result in enumerate(results):
            changes_list[result_mapping[task_idx]] = result
            
    return changes_list


async def _determine_write_permission(
    filename: str, 
    last_commit: Commit
) -> Tuple[bool, bool]:
    """Determine if a file should be writable based on its commit status.
    
    Args:
        filename: Path to the file.
        last_commit: The last commit for this file.
        
    Returns:
        Tuple of (should apply permission, should be writable)
    """
    repository = Repository.from_filename(os.path.dirname(filename))
    if not repository:
        return False, False
        
    spread = last_commit.commit_spread
    if not spread:
        return False, False
        
    is_uncommitted = spread == CommitSpread.MINE_UNCOMMITTED
    is_local = spread == CommitSpread.MINE_ACTIVE_BRANCH
    write_permission = is_uncommitted or is_local
    
    return True, write_permission


async def update_files_permissions(filenames: List[str]) -> None:
    """Update the permissions of multiple files in parallel.

    Args:
        filenames: List of relative or absolute filenames to update permissions for.
    """
    # Get last commits for all files
    last_commits = await get_files_last_commits(filenames)
    
    # Determine permissions for each file
    permission_tasks = []
    for filename, last_commit in zip(filenames, last_commits):
        task = _determine_write_permission(filename, last_commit)
        permission_tasks.append(task)
        
    permission_results = await asyncio.gather(*permission_tasks)
    
    # Set permissions in parallel
    chmod_tasks = []
    for filename, (should_update, write_permission) in zip(filenames, permission_results):
        if should_update:
            chmod_tasks.append(_set_write_permission(filename, write_permission))
            
    if chmod_tasks:
        await asyncio.gather(*chmod_tasks)


async def _set_write_permission(
    filename: str, write_permission: bool, safe: bool = False
) -> bool:
    """Set the write permission of a file asynchronously.

    Args:
        filename: The path to the file.
        write_permission: Whether to make the file writable.
        safe: Won't raise exceptions if the file doesn't exist or is not accessible.

    Returns:
        Whether the file ends in the desired state.
    """
    # Use asyncio.to_thread for potentially blocking IO operations
    try:
        # Get current permissions
        current_permissions = os.stat(filename).st_mode
        
        # Set new permissions based on write_permission
        if write_permission:
            new_permissions = current_permissions | stat.S_IWRITE
        else:
            new_permissions = current_permissions & ~stat.S_IWRITE
            
        # Apply new permissions
        os.chmod(filename, new_permissions)
        return True
        
    except FileNotFoundError:
        if safe:
            # If the file doesn't exist we can't set its permissions
            return False
        raise
        
    except PermissionError:
        if safe:
            # If we can't change permissions but want to make it read-only,
            # the operation effectively succeeded
            return not write_permission
        raise


async def get_updated_tracked_commits(
    repository: Repository, claims: Optional[List[str]] = None
) -> List[Dict[str, Any]]:
    """Get updated tracked commits.
    
    This function returns local commits for all clones with local commits
    and uncommitted changes from this clone.
    
    Args:
        repository: The repository to get tracked commits for.
        claims: List of files that we want to claim.
        
    Returns:
        List of tracked commits.
    """
    # Get remote URL once to avoid repeated lookups
    remote_url = repository.remote_url
    
    # Filter existing tracked commits
    tracked_commits = [
        commit for commit in repository.store.commits
        if commit.get("remote") != remote_url or not commit.is_issued_commit()
    ]
    
    # Add local commits
    local_commits = await get_local_only_commits(repository, claims=claims)
    tracked_commits.extend(local_commits)
    
    return tracked_commits


async def sync_tracked_commits(
    repository: Repository, claims: Optional[List[str]] = None
) -> None:
    """Pull the tracked commits from the store and update them.
    
    Args:
        repository: The repository to sync.
        claims: List of files that we want to claim.
    """
    # Update tracked commits
    repository.store.commits = await get_updated_tracked_commits(
        repository, claims=claims
    )
    
    # Update file permissions if configured
    if repository.config.get("modify_permissions", DEFAULT_MODIFY_PERMISSIONS):
        # Convert relative paths to absolute
        absolute_filenames = [
            repository.get_absolute_path(filename)
            for filename in repository.files
        ]
        
        # Update permissions
        if absolute_filenames:
            await repository.batch.update_files_permissions(absolute_filenames)


async def _process_commit_for_claim(last_commit: Commit) -> Commit:
    """Process a commit to determine if it's blocking for claiming a file.
    
    Args:
        last_commit: The commit to process.
        
    Returns:
        Either an empty commit (if file can be claimed) or the blocking commit.
    """
    spread = last_commit.commit_spread
    
    # Check if file is already mine (local or uncommitted)
    is_local_commit = (
        spread & CommitSpread.MINE_ACTIVE_BRANCH == CommitSpread.MINE_ACTIVE_BRANCH
    )
    is_uncommitted = (
        spread & CommitSpread.MINE_UNCOMMITTED == CommitSpread.MINE_UNCOMMITTED
    )
    
    # Return empty commit if file can be claimed, otherwise return blocking commit
    return Commit(None) if (is_local_commit or is_uncommitted) else last_commit


async def claim_files(
    filenames: List[str],
    prune: bool = True,
) -> List[Commit]:
    """Claim files for editing.
    
    If the file is available for changes, temporarily communicates files as changed.
    By communicate we mean the file will be marked as a local change until the next
    update of the tracked commits. Also makes the files writable if configured to
    affect permissions.

    Args:
        filenames: A list of absolute filenames to claim.
        prune: Prune branches if a fetch is necessary.

    Returns:
        The blocking commits for the files we want to claim.
    """
    # Get last commits for all files
    last_commits = await get_files_last_commits(filenames, prune=prune)
    
    # Process commits in parallel to determine if they're blocking
    tasks = [_process_commit_for_claim(commit) for commit in last_commits]
    blocking_commits = await asyncio.gather(*tasks)
    
    # Group files by repository
    filenames_by_repository: Dict[Repository, List[str]] = {}
    for filename in filenames:
        repository = Repository.from_filename(filename)
        if repository:
            filenames_by_repository.setdefault(repository, []).append(filename)
    
    # Update tracked commits for each repository in parallel
    sync_tasks = [
        sync_tracked_commits(repository, claims=repo_files)
        for repository, repo_files in filenames_by_repository.items()
        if repository
    ]
    
    if sync_tasks:
        await asyncio.gather(*sync_tasks)
        
    return blocking_commits
