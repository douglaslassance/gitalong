import os
import stat
import logging
import subprocess
import git


def is_binary_file(filename):
    textchars = bytearray({7, 8, 9, 10, 12, 13, 27} | set(range(0x20, 0x100)) - {0x7F})
    is_binary_string = lambda bytes: bool(bytes.translate(None, textchars))
    with open(filename, "rb") as fle:
        return is_binary_string(fle.read(1024))


def set_file_read_only(filename, read_only=True):
    if not os.path.exists(filename):
        logging.info("{} does not exist!".format(filename))
        return
    if read_only:
        os.chmod(filename, stat.S_IREAD)
        logging.info("{} was made read-only".format(filename))
        return
    os.chmod(filename, stat.S_IWRITE)
    logging.info("{} was made writable.".format(filename))


def find_repository_root(path):
    path = os.path.abspath(path)
    if os.path.isfile(path):
        path = os.path.dirname(path)
    path = os.path.normpath(path)
    cmd = ["git", "-C", path, "rev-parse", "--show-toplevel"]
    logging.debug(" ".join(cmd))
    output = subprocess.run(cmd, capture_output=True)
    path = output.stdout.decode("utf-8").replace("\n", "")
    path = os.path.normpath(path)
    if not os.path.exists(os.path.join(path, ".git")):
        logging.error('"{}" is not in a Git repository.'.format(path))
        return ""
    return path


def is_ci_runner_host() -> bool:
    """TODO: This currently only recognize GitHub runners.

    Returns:
        bool: True if the current host is a CI runner.
    """
    return "CI" in os.environ


def get_default_branch(repository: git.Repo) -> str:
    """TODO: We should be detecting that.

    Args:
        repository (git.Repo): The repository to get the default branch for.

    Returns:
        str: The default branch name.
    """
    return "main"


def get_real_path(filename: str) -> str:
    """
    Args:
        filename (str): The filemame for which we want the real path.

    Returns:
        str: Real path in case this path goes through Windows subst.
    """
    if not os.path.exists(filename):
        return ""
    # On Windows, this private function is available and will return the real path
    # for a subst location.
    if hasattr(os.path, "_getfinalepathname"):
        filename = os.path._getfinalepathname(filename)
        filename.replace("\\\\?\\", "")
    return filename


def get_remote_branches(repository: git.Repo, remote="origin") -> list:
    """
    Args:
        repository (git.Repo): The repository to check remote branches for.
        remote (str, optional): The name of the remote to check. Defaults to "origin".

    Returns:
        list: A list of branch name on the repository.
    """
    remote_branches = []
    for ref in repository.refs:
        if not ref.is_remote():
            continue
        splits = "/".split(ref.name)
        if len(splits) == 2 and splits[0] == remote:
            remote_branches.append(splits[-1])
    return remote_branches


def get_local_branches(repository: git.Repo) -> list:
    """
    Args:
        repository (git.Repo): The repository to check local branches for.

    Returns:
        list: A list of local branch names.
    """
    return [head.name for head in repository.heads]
