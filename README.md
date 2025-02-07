# gitalong

[![PyPI version](https://badge.fury.io/py/gitalong.svg)](https://badge.fury.io/py/gitalong)
[![Documentation Status](https://readthedocs.org/projects/gitalong/badge/?version=latest)](https://gitalong.readthedocs.io/en/latest)
[![codecov](https://codecov.io/gh/douglaslassance/gitalong/branch/main/graph/badge.svg?token=5267NA3EQQ)](https://codecov.io/gh/douglaslassance/gitalong)

Gitalong is a tool for Git repositories that seek to prevent conflicts between files when working with a team.
It uses hooks and a store to communicate local changes across all clones of a given remote.
In turns this information can be leveraged by integrations to prevent modifying files that are already changed
elsewhere.

## Pre-requisites

- [Python >=3.10](https://www.python.org/downloads/)
- [Git >=2.35.1](https://git-scm.com/downloads)

> [!TIP]
> Setting up Python and Git can be intimidating on Windows. You can make your life easier by installing [Scoop](https://scoop.sh/) and running `scoop install python git` in a Windows command prompt.

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
gitalong -C project setup store.git --modify-permissions --tracked-extensions .jpg,.png --track-uncommitted --update-gitignore --update-hooks

# Creating some files.
touch project/untracked.txt
touch project/uncommitted.png
touch project/local.png
touch project/current.jpg
touch project/remote.jpg

# Spreading them across branches.
git -C project add current.jpg
git -C project commit -m "Add current.jpg"
git -C project add remote.jpg
git -C project commit -m "Add remote.jpg"
git -C project push
git -C project reset --hard HEAD^
git -C project add local.png
git -C project commit -m "Add local.png"

# Updating tracked commits with current local changes.
# Because we passed `--track-uncommitted`, uncommitted changes will be stored as sha-less commit.
# Because we passed `--modify-permssions` the file permissions will be updated.
# When passing `--update-hooks`, the update will happen automatically on the following hooks:
# applypatch, post-checkout, post-commit, post-rewrite.
gitalong -C project update

# Checking the status for the files we created.
# Each status will show <spread> <filename> <commit> <local-branches> <remote-branches> <host> <author>.
# Spread flags represent where the commit lives. It will be displayed in the following order:
# <mine-uncommitted><mine-active-branch><mine-other-branch><remote-matching-branch><remote-other-branch><other-other-branch><other-matching-branch><other-uncomitted>
# A `+` sign means is true, while a `-` sign means false or unknown.
gitalong -C project status untracked.txt uncommited.png local.png current.jpg remote.jpg

# Claiming the files to modify them.
# If the file cannot be claimed the "blocking" commit will be returned.
# Since we passed `--modify-permissions`, the claimed file will be made writeable.
# These claimed files will be released automatically on the next update if not modified.
gitalong -C project claim untracked.txt uncommited.png local.png current.jpg remote.jpg
```

### Python

You can find a usage example in [example.py](https://github.com/douglaslassance/gitalong/blob/main/tests/example.py).

## Stores

As mentioned earlier, Gitalong needs an accessible place to store and share local changes with all clones of the managed
repository.
Multiple options are offered here.

### Git repository

A Git repository can be used for this purpose.
The advantage of this solution is that you won't need more infrastructure and security mechanisms than what is needed to
access your project's repository. That said, pulling and pushing the data that way is pretty slow.
This method is used in the usage examples above.

### JSONBin.io

[JSONBin.io](https://jsonbin.io) is a simple JSON hosting service.
You will get faster operations with this option but it may come at a [cost](https://jsonbin.io/pricing) depending on
your usage. See how to set this up below.

### Shell

```azure
gitalong -C project setup https://api.jsonbin.io/v3/b/<BIN_ID> --store-header X-Access-Key=<ACCESS_KEY> ...
```

### Python

```python
repository = Repository.setup(
    store_url="https://api.jsonbin.io/v3/b/<BIN_ID>",
    store_headers={"X-Access-Key": "<ACCESS_KEY>"},
    ...
```

Worth noting that `<ACCESS_KEY>` can be an environment variable such as `$ACCESS_KEY`.

## Development

### Setting up

Setup a Python virtual environment and run the following command.

```shell
python -m venv .venv
source .venv/bin/activate
pip install -e ".[ci]"
```

### Testing

```shell
pytest - -cov - report = html - -cov = gitalong - -profile - svg
```

### Documenting

```shell
sphinx-build ./docs/source ./docs/build
```

### Building

```shell
python setup.py sdist bdist_wheel
```

### Publishing

```shell
twine upload --username __token__ --verbose dist/*
```
