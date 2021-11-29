# gitarmony-python

[![PyPI version](https://badge.fury.io/py/gitarmony-python.svg)](https://badge.fury.io/py/gitarmony-python)
[![Documentation Status](https://readthedocs.org/projects/gitarmony-python/badge/?version=latest)](https://gitarmony-python.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitarmony-python/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitarmony-python)

A Python API allowing to interact with Gitarmony features on a Git repository.
More about Gitarmony in this [medium article]().

## Usage

```python
from gitarmony import Gitarmony, GitarmonyNotInstalled

try:
    gitarmony = Gitarmony(managed_repository_path)
except GitarmonyNotInstalled:
    # Gitarmony stores its data in its own repository therefore we need to pass that repository URL.
    gitarmony = Gitarmony.install(managed_repository_path, data_repository_url)

change_list = gitarmony.change_list
change_list.synchronize()
```

# Development

This projects requires the following:

-   [Python >=3.7.9](https://www.python.org/downloads/release/python-379/)
-   [virtualenwrapper](https://pypi.org/project/virtualenvwrapper/) (macOS/Linux)
-   [virtualenwrapper-win](https://pypi.org/project/virtualenvwrapper-win/) (Windows)

Make sure your `WORKON_HOME` environment variable is set on Windows, and create a `gitarmony-python` virtual environment with `mkvirtualenv`.
Build systems for installing requirements and running tests are on board of the SublimeText project.
