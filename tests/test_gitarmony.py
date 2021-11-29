import sys
import os
import shutil
import tempfile

from git import Repo
from gitarmony.gitarmony import Gitarmony
from gitarmony.functions import get_real_path
from PIL import Image


def test_gitarmony():
    temp_dir = tempfile.mkdtemp()
    managed_origin = Repo.init(path=os.path.join(temp_dir, "managed.git"), bare=True)
    managed_clone = managed_origin.clone(os.path.join(temp_dir, "managed"))
    managed_origin_URL = os.path.join(temp_dir, "origin.git")
    data_origin = Repo.init(path=managed_origin_URL, bare=True)
    data_clone = data_origin.clone(
        os.path.join(managed_clone.working_dir, ".gitarmony")
    )

    image = Image.new(mode="RGB", size=(256, 256))
    image_path = os.path.join(managed_clone.working_dir, "image.jpg")
    image.save(image_path, "JPEG")
    managed_clone.index.add(image_path)

    gitarmony = Gitarmony(managed_clone.working_dir)
    assert len(gitarmony.local_commits) == 0, "Local commit count should be zero!"
