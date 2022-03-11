"""API to perform Gitalong operations on a Git repository."""

import os
import logging

from dotenv import load_dotenv

from .__info__ import __version__, __copyright__, __email__, __author__  # noqa: F401
from .gitalong import Gitalong  # noqa: F401
from .enums import CommitSpread  # noqa: F401
from .exceptions import GitalongNotInstalled  # noqa: F401


# Performing global setup.
load_dotenv()
logging.getLogger().setLevel(os.environ.get("GITARMONY_PYTHON_DEBUG_LEVEL", "INFO"))