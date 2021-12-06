import os
import stat
import pathlib


def is_binary_file(filename):
    textchars = bytearray({7, 8, 9, 10, 12, 13, 27} | set(range(0x20, 0x100)) - {0x7F})
    is_binary_string = lambda bytes: bool(bytes.translate(None, textchars))
    with open(filename, "rb") as fle:
        return is_binary_string(fle.read(1024))


def is_read_only(filename) -> bool:
    """TODO: Make sure this works on other operating system than Windows.

    Args:
        filename (TYPE): The absolute filename of the file to check.

    Returns:
        bool: Whether the file is read only.
    """
    _stat = os.stat(filename)
    return not bool(_stat.st_mode & stat.S_IWRITE)


def set_read_only(filename, read_only=True, check_exists=True):
    """Sets the file read-only state.

    Args:
        filename (TYPE): The absolute filename of the file we want to set.
        read_only (bool, optional): Whether read-only should be true of false.
        check_exists (bool, optional): Whether we are guarding from non existing files.
    """
    if check_exists and not os.path.exists(filename):
        return
    if read_only:
        os.chmod(filename, stat.S_IREAD)
        return
    os.chmod(filename, stat.S_IWRITE)


def is_ci_runner_host() -> bool:
    """TODO: This currently only recognize GitHub runners.

    Returns:
        bool: True if the current host is a CI runner.
    """
    return "CI" in os.environ


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
        filename = os.path._getfinalpathname(filename)
        filename = str(pathlib.Path(filename).resolve())
    return filename
