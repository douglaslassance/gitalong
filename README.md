# gitarmony-python

[![PyPI version](https://badge.fury.io/py/gitarmony-python.svg)](https://badge.fury.io/py/gitarmony-python)
[![Documentation Status](https://readthedocs.org/projects/gitarmony-python/badge/?version=latest)](https://gitarmony-python.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitarmony-python/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitarmony-python)

A Python API allowing to interact with Gitarmony features on a Git repository.
More about Gitarmony in this [medium article]().

## Pre-requisites

-   [Git >=2.35.1](https://git-scm.com/downloads)

## Installation

```
pip install gitarmony
```

## Usage

```python
from pprint import pprint

from gitarmony import Gitarmony, GitarmonyNotInstalled

try:
    gitarmony = Gitarmony(managed_repository_path)
except GitarmonyNotInstalled:
    # Gitarmony stores its data in its own repository therefore we need to pass that repository URL.
    gitarmony = Gitarmony.install(managed_repository_path, data_repository_url)

# Now we'll get the last commit for a given file.
# This could return a dummy commit representing uncommitted changes.
last_commit = gitarmony.get_file_last_commit(filename)
pprint(last_commit)

spread = gitarmony.get_commit_spread(commit)
if commit_spread & CommitSpread.LOCAL_UNCOMMITTED == CommitSpread.LOCAL_UNCOMMITTED:
    print("Commit represents our local uncommitted changes."
if commit_spread & CommitSpread.LOCAL_ACTIVE_BRANCH == CommitSpread.LOCAL_ACTIVE_BRANCH:
    print("Commit is on our local active branch."
if commit_spread & CommitSpread.LOCAL_OTHER_BRANCH == CommitSpread.LOCAL_OTHER_BRANCH:
    print("Commit is in one ore more of our other local branches."
if commit_spread & CommitSpread.REMOTE_MATCHING_BRANCH == CommitSpread.REMOTE_MATCHING_BRANCH:
    print("Commit is on the matching remote branch."
if commit_spread & CommitSpread.REMOTE_OTHER_BRANCH == CommitSpread.REMOTE_OTHER_BRANCH:
    print("Commit is one ore more other remote branches."
if commit_spread & CommitSpread.CLONE_OTHER_BRANCH == CommitSpread.CLONE_OTHER_BRANCH:
    print("Commit is on someone else's clone non-matching branch."
if commit_spread & CommitSpread.CLONE_MATCHING_BRANCH == CommitSpread.CLONE_MATCHING_BRANCH:
    print("Commit is on another clone's matching branch."
if commit_spread & CommitSpread.CLONE_UNCOMMITTED == CommitSpread.CLONE_UNCOMMITTED:
    print("Commit represents someone else's uncommitted changes."

# To update tracked commit with the ones based on local changes.
gitarmony.update_tracked_commits()

# To update permissions of tracked files.
gitarmony.update_binary_permissions()
```

# Development

This projects requires the following:

-   [Python >=3.7](https://www.python.org/downloads/)
-   [virtualenwrapper](https://pypi.org/project/virtualenvwrapper/) (macOS/Linux)
-   [virtualenwrapper-win](https://pypi.org/project/virtualenvwrapper-win/) (Windows)

Make sure your `WORKON_HOME` environment variable is set on Windows, and create a `gitarmony-python` virtual environment with `mkvirtualenv`.
Build systems for installing requirements and running tests are on board of the SublimeText project.
