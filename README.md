# gitarmony-python

[![PyPI version](https://badge.fury.io/py/gitarmony-python.svg)](https://badge.fury.io/py/gitarmony-python)
[![Documentation Status](https://readthedocs.org/projects/gitarmony-python/badge/?version=latest)](https://gitarmony-python.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitarmony-python/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitarmony-python)

A Python API allowing to interact with gitarmony features on a Git repository.
More about gitarmony in this [medium article]().

## Usage

```python
from gitarmony import Gitarmony
from gitarmony.exceptions import GitarmonyNotInstalled

try:
    gitarmony = Gitarmony(managed_repository_path)
except GitarmonyNotInstalled:
    # Gitarmony stores its data in its own repository therefore we need to pass a repository URL.
    gitarmony = Gitarmony.install(managed_repository_path, data_repository_url)

change_list = gitarmony.change_list
change_list.synchronize()
```
