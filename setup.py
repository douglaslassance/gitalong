"""Setup for gitalong.
"""

import os

from setuptools import setup

dirname = os.path.dirname(__file__)
info = {}
with open(os.path.join(dirname, "gitalong", "__info__.py"), mode="r") as f:
    exec(f.read(), info)  # pylint: disable=W0122

# Get the long description from the README file.
with open(os.path.join(dirname, "README.md"), encoding="utf-8") as fle:
    long_description = fle.read()

setup(
    name="gitalong",
    version=info.get("__version__", ""),
    description=("Git without conflicts."),
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/douglaslassance/gitalong-python",
    author=info.get("__author__", ""),
    author_email=info.get("__email__", ""),
    license=info.get("__license__", ""),
    packages=["gitalong"],
    install_requires=[
        "click~=8.0",
        "GitPython~=3.1",
        "dictdiffer~=0.9",
    ],
    extras_require={
        "ci": [
            "black",
            "click~=8.0",
            "flake8-print~=3.1",
            "flake8~=3.9",
            "pep8-naming~=0.11",
            "Pillow~=8.4",
            "pylint~=2.9",
            "pytest-cov~=2.12",
            "pytest-html~=2.1",
            "pytest-pep8~=1.0",
            "pytest-profiling~=1.7",
            "sphinx-markdown-tables~=0.0",
            "sphinx-rtd-theme~=0.5",
            "sphinxcontrib-apidoc~=0.3",
            "Sphinx~=4.5",
        ],
    },
    entry_points={
        "console_scripts": [
            "gitalong = gitalong.cli:main",
        ],
        "gui_scripts": [
            "gitalong-gui = gitalong.cli:main",
        ],
    },
    include_package_data=True,
    python_require="~=3.7",
)
