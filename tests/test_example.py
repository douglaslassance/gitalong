import os
import tempfile
import logging

from pprint import pprint

from git.repo import Repo
from gitalong import Repository, RepositoryNotSetup, CommitSpread


def test_example():
    # Creating temp directory for the files.
    dirname = tempfile.mkdtemp()
    logging.info(dirname)

    # Creating a dummy project repository and its clone in temp directory.
    project = Repo.init(path=os.path.join(dirname, "project.git"), bare=True)
    project_clone = project.clone(os.path.join(dirname, "project"))

    try:
        # This will raise as we never setup that repository with Gitalong.
        repository = Repository(project_clone.working_dir)

    except RepositoryNotSetup:

        # Creating a repository that Gitalong will use to store and share local changes. You
        # would normally host this somewhere like GitHub so your entire team has access to
        # it.
        store = Repo.init(path="store.git", bare=True)

        # Setting up Gitalong in your project repository.
        # This will clone the registry repository in an ignored `.gitalong` folder.
        # It will also start tracking a `.gitalong.json` configuration file.
        repository = Repository.setup(
            store_repository=store.working_dir,
            managed_repository=project_clone.working_dir,
            modify_permissions=True,
            tracked_extensions=[".jpg", ".gif", ".png"],
            track_uncommitted=True,
            update_gitignore=True,
            # Skipping hook update for the test.
            update_hooks=False,
        )

    # Creating some files.
    open(os.path.join(project_clone.working_dir, "uncommitted.png"), "w").close()
    open(os.path.join(project_clone.working_dir, "local.gif"), "w").close()
    open(os.path.join(project_clone.working_dir, "remote.jpg"), "w").close()
    open(os.path.join(project_clone.working_dir, "untracked.txt"), "w").close()

    # Spreading them across branches.
    project_clone.index.add("untracked.txt")
    project_clone.index.commit(message="Add untracked.txt")
    project_clone.index.add("remote.jpg")
    project_clone.index.commit(message="Add remote.jpg")
    project_clone.remote().push()
    project_clone.git.reset("--hard", "HEAD^")
    project_clone.index.add("local.gif")
    project_clone.index.commit(message="Add local.gif")

    # Updating tracked commits with current local changes. Because we specified
    # `track_uncommitted`. Uncommitted changes will be stored as sha-less commit.
    repository.update_tracked_commits()

    # Update permissions of all files based on track commits. Because `modify_permssions`
    # was passed this will update all permissions of tracked files. Permission updates
    # currently comes at high performance cost and is not recommended.
    locally_changed_files = repository.locally_changed_files
    for filename in repository.files:
        repository.update_file_permissions(filename, locally_changed_files)

    # Now we'll get the last commit for our files.
    # This could return a dummy commit representing uncommitted changes.
    uncommitted_last_commit = repository.get_file_last_commit("uncommitted.png")
    local_last_commit = repository.get_file_last_commit("local.gif")
    remote_last_commit = repository.get_file_last_commit("remote.jpg")
    untracked_last_commit = repository.get_file_last_commit("untracked.txt")

    # Getting the commit spreads.
    # Spread flags represent where the commit live.
    uncommitted_spread = repository.get_commit_spread(uncommitted_last_commit)
    local_spread = repository.get_commit_spread(local_last_commit)
    remote_spread = repository.get_commit_spread(remote_last_commit)
    untracked_spread = repository.get_commit_spread(untracked_last_commit)

    assert uncommitted_spread == CommitSpread.MINE_UNCOMMITTED
    assert local_spread == CommitSpread.MINE_ACTIVE_BRANCH
    assert remote_spread == CommitSpread.REMOTE_MATCHING_BRANCH
    assert untracked_spread == (
        CommitSpread.REMOTE_MATCHING_BRANCH | CommitSpread.MINE_ACTIVE_BRANCH
    )

    # Trying to make the files writable.
    assert bool(repository.make_file_writable("uncommitted.png")) == False
    assert bool(repository.make_file_writable("local.gif")) == False
    assert bool(repository.make_file_writable("remote.jpg")) == True
    assert bool(repository.make_file_writable("untracked.txt")) == False


if __name__ == "__main__":
    test_example()
