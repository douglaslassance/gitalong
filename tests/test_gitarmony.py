import sys
import os
import tempfile

from git import Repo
from gitarmony.gitarmony import Gitarmony

TEMP_DIR = tempfile.mkdtemp()
MANAGED_ORIGIN = Repo.init(path=os.path.join(TEMP_DIR, "managed.git"), bare=True)
MANAGED_CLONE = MANAGED_ORIGIN.clone(os.path.join(TEMP_DIR, "managed"))
DATA_ORIGIN = Repo.init(path=os.path.join(TEMP_DIR, "origin.git"), bare=True)
DATA_CLONE = DATA_ORIGIN.clone(os.path.join(MANAGED_CLONE.working_dir, ".gitarmony"))


def test_main():
    gitarmony = Gitarmony(MANAGED_CLONE.working_dir)
    print("LOL", set(gitarmony.change_set))
    assert gitarmony.change_set == set()
