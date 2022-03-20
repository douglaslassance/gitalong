# gitalong-python

[![PyPI version](https://badge.fury.io/py/gitalong.svg)](https://badge.fury.io/py/gitalong)
[![Documentation Status](https://readthedocs.org/projects/gitalong/badge/?version=latest)](https://gitalong.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitalong-python/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitalong-python)

An API built-on top of Git to avoid conflicts when working with others.

## Pre-requisites

-   [Git >=2.35.1](https://git-scm.com/downloads)

## Installation

```
pip install gitalong
```

## Usage

```python
from pprint import pprint

from gitalong import Gitalong, GitalongNotInstalled

try:
    gitalong = Gitalong(managed_repository_path)
except GitalongNotInstalled:
    # Gitalong stores its data in its own repository therefore we need to pass that repository URL.
    gitalong = Gitalong.install(managed_repository_path, data_repository_url)

# Now we'll get the last commit for a given file.
# This could return a dummy commit representing uncommitted changes.
last_commit = gitalong.get_file_last_commit(filename)
pprint(last_commit)

spread = gitalong.get_commit_spread(commit)
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
gitalong.update_tracked_commits()

# To update permissions of tracked files.
gitalong.update_binary_permissions()
```

# Development

This projects requires the following:

-   [Python >=3.7](https://www.python.org/downloads/)
-   [virtualenwrapper](https://pypi.org/project/virtualenvwrapper/) (macOS/Linux)
-   [virtualenwrapper-win](https://pypi.org/project/virtualenvwrapper-win/) (Windows)

Make sure your `WORKON_HOME` environment variable is set on Windows, and create a `gitalong-python` virtual environment with `mkvirtualenv`.
Build systems for installing requirements and running tests are on board of the SublimeText project.
