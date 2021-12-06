import os
import logging

import coloredlogs

from dotenv import load_dotenv

from .__info__ import __version__, __copyright__, __email__, __author__
from .gitarmony import Gitarmony
from .exceptions import GitarmonyNotInstalled


# Performing global setup.
load_dotenv()
logging.getLogger().setLevel(os.environ.get("GITARMONY_PYTHON_DEBUG_LEVEL", "INFO"))
