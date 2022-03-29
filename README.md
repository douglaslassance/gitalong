# gitalong

[![PyPI version](https://badge.fury.io/py/gitalong.svg)](https://badge.fury.io/py/gitalong)
[![Documentation Status](https://readthedocs.org/projects/gitalong/badge/?version=latest)](https://gitalong.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitalong/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitalong)

Gitalong is a tool for Git repositories that seek to prevent conflicts between files when working with a team.
It uses hooks and a dedicated repository to communicate local changes across all clones of a given remote.
In turns this information can be leveraged by integrations to prevent modifying file that are already changed elsewhere.

## Pre-requisites

-   [Python >=3.7](https://www.python.org/downloads/)
-   [Git >=2.35.1](https://git-scm.com/downloads)

## Installation

```
pip install gitalong
```

## Usage

### Shell

```shell
# Creating a dummy project repository and its clone in current working directory.
git init --bare project.git
git clone project.git project

# Creating a repository that Gitalong will use to store and share local changes.
# You would normally host this somewhere like GitHub so your entire team has access to it.
git init --bare store.git

# Setting up Gitalong in your project repository.
# This will clone the store repository in an ignored `.gitalong` folder.
# It will also start tracking a `.gitalong.json` configuration file.
gitalong -C project setup store.git --modify-permissions --tracked-extensions .jpg,.gif,.png --track-uncommitted --update-gitignore --update-hooks

# Creating some files.
touch project/uncommitted.png
touch project/local.gif
touch project/remote.jpg
touch project/untracked.txt

# Spreading them across branches.
git -C project add untracked.txt
git -C project commit -m "Add untracked.txt"
git -C project add remote.jpg
git -C project commit -m "Add remote.jpg"
git -C project push
git -C project reset --hard HEAD^
git -C project add local.gif
git -C project commit -m "Add local.gif"

# Updating tracked commits with current local changes.
# Because we specified `track_uncommitted`. Uncommitted changes will be stored as sha-less commit.
# Update permissions of all files based on track commits.
# Because `modify_permssions` was passed this will update all permissions of tracked files.
# Permission updates currently comes at high performance cost and is not recommended.
gitalong -C project update

# Checking the status for the files we created.
# Each status will show <spread> <filename> <commit> <local-branches> <remote-branches> <host> <author>.
# Spread flags represent where the commit live.
# It will be displayed in the following order:
# <local-uncommitted><local-active-branch><local-other-branch><remote-matching-branch><remote-other-branch><clone-other-branch><clone-matching-branch><clone-uncomitted>
# A `+` sign means is true, while a `-` sign means false or unknown.
gitalong -C project status uncommited.jpg local.gif remote.jpg untracked.txt

# If you installed with `--modify-permissions` this will try to make the files writable.
# The command will return and error code of 1 if one ore more of the files cannot be made writable.
gitalong -C project claim uncommited.jpg local.gif remote.jpg untracked.txt
```

### Python

```python
import os
import tempfile
import logging

from git.repo import Repo
from gitalong import Repository, RepositoryNotSetup, CommitSpread


def test_example():
    """Testing example code featured in README.md."""
    dirname = tempfile.mkdtemp()
    logging.info(dirname)

    # Creating a dummy project repository and its clone in temp directory.
    project = Repo.init(path=os.path.join(dirname, "project.git"), bare=True)
    project_clone = project.clone(os.path.join(dirname, "project"))

    try:
        # This will raise as we never setup that repository with Gitalong.
        repository = Repository(project_clone.working_dir)

    except RepositoryNotSetup:

        # Creating a repository that Gitalong will use to store and share local changes.
        # You would normally host this somewhere like GitHub so your entire team has
        # access to it.
        store = Repo.init(path=os.path.join(dirname, "store.git"), bare=True)

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

    # Update permissions of all files based on track commits. Because
    # `modify_permssions` was passed this will update all permissions of tracked files.
    # Permission updates currently comes at high performance cost and is not
    # recommended.
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
    assert bool(repository.make_file_writable("uncommitted.png")) is False
    assert bool(repository.make_file_writable("local.gif")) is False
    assert bool(repository.make_file_writable("remote.jpg")) is True
    assert bool(repository.make_file_writable("untracked.txt")) is False


if __name__ == "__main__":
    test_example()

```

# Development

This projects requires the following:

-   [Python >=3.7](https://www.python.org/downloads/)
-   [virtualenwrapper](https://pypi.org/project/virtualenvwrapper/) (macOS/Linux)
-   [virtualenwrapper-win](https://pypi.org/project/virtualenvwrapper-win/) (Windows)

Make sure your `WORKON_HOME` environment variable is set on Windows, and create a `gitalong` virtual environment with `mkvirtualenv`.
Build systems for installing requirements and running tests are on board of the SublimeText project.
