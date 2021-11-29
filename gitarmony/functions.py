import os
import stat
import logging
import pathlib


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
    if not os.path.exists(filename):
        return ""
    # On Windows, this private function is available and will return the real path
    # for a subst location.
    if hasattr(os.path, "_getfinalepathname"):
        filename = os.path._getfinalepathname(filename)
        filename = pathlib.Path(filename).resolve()
    return filename
