import os
import time
import stat
import pathlib

import git


def is_binary_file(filename: str) -> bool:
    """
    Args:
        filename (str): The path to the file to analyze.

    Returns:
        bool: Whether the file is a binary.
    """
    with open(filename, "rb") as fle:
        return is_binary_string(fle.read(1024))
    return False


def is_binary_string(string: str) -> bool:
    """
    Args:
        string (str): A string to analyze.

    Returns:
        bool: Whether the string is a binary string.
    """
    textchars = bytearray({7, 8, 9, 10, 12, 13, 27} | set(range(0x20, 0x100)) - {0x7F})
    return bool(string.translate(None, textchars))


def is_read_only(filename: str) -> bool:
    """TODO: Make sure this works on other operating system than Windows.

    Args:
        filename (str): The absolute filename of the file to check.

    Returns:
        bool: Whether the file is read only.
    """
    _stat = os.stat(filename)
    return not bool(_stat.st_mode & stat.S_IWRITE)


def set_read_only(filename: str, read_only: bool = True, check_exists: bool = True):
    """Sets the file read-only state.

    Args:
        filename (str): The absolute filename of the file we want to set.
        read_only (bool, optional): Whether read-only should be true of false.
        check_exists (bool, optional): Whether we are guarding from non existing files.
    """
    if check_exists and not os.path.exists(filename):
        return
    if read_only:
        os.chmod(filename, stat.S_IREAD)
        return
    os.chmod(filename, stat.S_IWRITE)


def get_real_path(filename: str) -> str:
    """
    Args:
        filename (str): The filemame for which we want the real path.

    Returns:
        str: Real path in case this path goes through Windows subst.
    """
    # On Windows, this private function is available and will return the real path
    # for a subst location.
    if hasattr(os.path, "_getfinalpathname"):
        filename = os.path._getfinalpathname(  # pylint: disable=protected-access
            filename
        )
        filename = str(pathlib.Path(filename).resolve())
    return filename


def pulled_within(repository: git.Repo, seconds: float) -> bool:
    """Summary

    Args:
        repository (git.Repo): The repository to check for.
        seconds (float): Time in seconds since last push.

    Returns:
        TYPE: Whether the repository pulled within the time provided.
    """
    fetch_head = os.path.join(repository.git_dir, "FETCH_HEAD")
    if not os.path.exists(fetch_head):
        return False
    since_last = time.time() - os.path.getmtime(fetch_head)
    return seconds > since_last
