# pylint: disable=too-many-locals, too-many-statements, consider-using-with, line-too-long

import os
import tempfile
import logging
import asyncio

from git.repo import Repo
from gitalong import Repository, RepositoryNotSetup, CommitSpread, functions, batch


def example():
    """Usage example"""
    dirname = tempfile.mkdtemp()
    logging.info(dirname)

    # Creating a dummy project repository and its clone in temp directory.
    project = Repo.init(path=os.path.join(dirname, "project.git"), bare=True)
    project_clone = project.clone(os.path.join(dirname, "project"))

    try:
        # This will raise as we never setup that repository with Gitalong.
        repository = Repository(str(project_clone.working_dir))

    except RepositoryNotSetup:
        # Creating a repository that Gitalong will use to store and share local changes.
        # You would normally host this somewhere like GitHub so your entire team has access to it.
        store = Repo.init(path=os.path.join(dirname, "store.git"), bare=True)

        # Setting up Gitalong in your project repository.
        # This will clone the registry repository in an ignored `.gitalong` folder.
        # It will also start tracking a `.gitalong.json` configuration file.
        repository = Repository.setup(
            store_url=str(store.working_dir),
            managed_repository=str(project_clone.working_dir),
            modify_permissions=True,
            tracked_extensions=[".jpg", ".png"],
            track_uncommitted=True,
            update_gitignore=True,
            # Skipping hook updates for the test.
            update_hooks=False,
        )

    # Creating some files.
    untracked = os.path.join(project_clone.working_dir, "untracked.txt")
    uncommitted = os.path.join(project_clone.working_dir, "uncommitted.png")
    local = os.path.join(project_clone.working_dir, "local.png")
    current = os.path.join(project_clone.working_dir, "current.jpg")
    remote = os.path.join(project_clone.working_dir, "remote.jpg")
    open(uncommitted, "w", encoding="utf-8").close()
    open(local, "w", encoding="utf-8").close()
    open(current, "w", encoding="utf-8").close()
    open(remote, "w", encoding="utf-8").close()
    open(untracked, "w", encoding="utf-8").close()

    # Spreading them across branches.
    project_clone.index.add("current.jpg")
    project_clone.index.commit(message="Add current.jpg")
    project_clone.index.add("remote.jpg")
    project_clone.index.commit(message="Add remote.jpg")
    project_clone.remote().push()
    project_clone.git.reset("--hard", "HEAD^")
    project_clone.index.add("local.png")
    project_clone.index.commit(message="Add local.png")

    # Updating tracked commits with current local changes. Because we set
    # `track_uncommitted` to `True`, uncommitted changes will be stored as sha-less commit.
    # Because we specificed `update_permissions` to `True`, the file permissions will be updated.
    # When setting `update_hooks` to 'True', the update will happen automatically on the following hooks:
    # applypatch, post-checkout, post-commit, post-rewrite.
    asyncio.run(batch.update_tracked_commits(repository))

    # Checking the status for the files we created.
    # For that purpose, we'll get the last commit for our files.
    # Because we have set `track_uncomitted` to True, this will return a dummy commit for any uncommitted changes.
    last_commits = asyncio.run(
        batch.get_files_last_commits([untracked, uncommitted, local, current, remote])
    )
    untracked_last_commit = last_commits[0]
    uncommitted_last_commit = last_commits[1]
    local_last_commit = last_commits[2]
    current_last_commit = last_commits[3]
    remote_last_commit = last_commits[4]

    # Getting the commit spreads.
    # Spread flags represent where the commit live.
    untracked_spread = untracked_last_commit.commit_spread
    uncommitted_spread = uncommitted_last_commit.commit_spread
    local_spread = local_last_commit.commit_spread
    current_spread = current_last_commit.commit_spread
    remote_spread = remote_last_commit.commit_spread

    # Checking the resulting spreads.
    assert uncommitted_spread == CommitSpread.MINE_UNCOMMITTED
    assert local_spread == CommitSpread.MINE_ACTIVE_BRANCH
    assert (
        current_spread
        == CommitSpread.MINE_ACTIVE_BRANCH | CommitSpread.REMOTE_MATCHING_BRANCH
    )
    assert remote_spread == CommitSpread.REMOTE_MATCHING_BRANCH
    assert untracked_spread == 0

    # Claiming the files to modify them.
    # If the file cannot be claimed the "blocking" commit will be returned.
    # Since we have set `modify_permissions` to True, the claimed file will be made writeable.
    # These claimed files will be released automatically on the next update if not modified.
    blocking_commits = asyncio.run(
        repository.batch.claim_files([untracked, uncommitted, local, current, remote])
    )

    # Checking that we get the expected blocking commits.
    # The remote file is the only one that we are technically not allowed to change
    # since we don't have that change.
    assert not blocking_commits[0]
    assert not blocking_commits[1]
    assert not blocking_commits[2]
    assert not blocking_commits[3]
    assert blocking_commits[4]

    # Checking the write permissions.
    # Only the remote file should remain read-only.
    assert functions.is_writeable(untracked)
    assert functions.is_writeable(uncommitted)
    assert functions.is_writeable(local)
    assert functions.is_writeable(current)
    assert not functions.is_writeable(remote)


if __name__ == "__main__":
    example()
