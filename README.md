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

# We also provide a way to claim files so no one else can edit them.
# If you installed with `--modify-permissions` it will make the files writable.
# Each claim will show a non valid commit status if the claim was successful and a valid one if a commit prevented the claim.
gitalong -C project claim uncommited.jpg local.gif remote.jpg untracked.txt

# You can also release these claims.
# If you installed with `--modify-permissions` it will make the files read-only.
# Each release will show a non valid commit status if the release was successful and a valid one if a commit prevented the release.
gitalong -C project release uncommited.jpg local.gif remote.jpg untracked.txt
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
pip install --editable .[ci]
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
