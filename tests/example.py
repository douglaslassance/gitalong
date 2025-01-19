# pylint: disable=too-many-locals, too-many-statements, consider-using-with

import os
import tempfile
import logging
import asyncio

from git.repo import Repo
from gitalong import Repository, RepositoryNotSetup, CommitSpread


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
        # You would normally host this somewhere like GitHub so your entire team has
        # access to it.
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

    # Updating tracked commits with current local changes. Because we specified
    # `track_uncommitted`. Uncommitted changes will be stored as sha-less commit.
    # File permissions of files will also be updated.
    # This is expensive so only use that feature if you really need it.
    repository.update_tracked_commits()

    last_commits = asyncio.run(
        repository.batch.get_files_last_commits(
            [untracked, uncommitted, local, current, remote]
        )
    )

    # Now we'll get the last commit for our files.
    # This could return a dummy commit representing uncommitted changes.
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

    # Checking that the spread flags are correct.
    assert uncommitted_spread == CommitSpread.MINE_UNCOMMITTED
    assert local_spread == CommitSpread.MINE_ACTIVE_BRANCH
    assert (
        current_spread
        == CommitSpread.MINE_ACTIVE_BRANCH | CommitSpread.REMOTE_MATCHING_BRANCH
    )
    assert remote_spread == CommitSpread.REMOTE_MATCHING_BRANCH
    assert untracked_spread == 0

    # # We also provide a way to claim files so no one else can edit them.
    # # If you installed with `--modify-permissions` it will make the files writable.
    # claims = asyncio.run(
    #     repository.batch.claim_files([untracked, uncommitted, local, current, remote])
    # )

    # # Failed claims will return a valid commit that prevented the claim.
    # # In other words a successful claim will return an invalid commit.
    # assert bool(claims[0]) is True
    # assert bool(claims[1]) is True
    # assert bool(claims[2]) is True
    # assert bool(claims[3]) is False
    # assert bool(claims[4]) is True

    # # You can also release these claims.
    # # If you installed with `--modify-permissions` it will make the files read-only.
    # releases = asyncio.run(
    #     repository.batch.release_files([untracked, uncommitted, local, current, remote])
    # )

    # # Failed releases will return a valid commit that prevented the release.
    # # In other words a successful release will return an invalid commit.
    # assert bool(releases[0]) is True
    # assert bool(releases[1]) is True
    # assert bool(releases[2]) is True
    # assert bool(releases[3]) is False
    # assert bool(releases[4]) is True


if __name__ == "__main__":
    example()
